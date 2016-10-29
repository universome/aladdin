use std::cmp;
use std::thread;
use std::time::Duration;
use std::sync::Mutex;
use std::sync::atomic::{AtomicIsize, AtomicUsize};
use std::sync::atomic::Ordering::Relaxed;
use std::collections::HashMap;
use time;

use constants::{MIN_RETRY_DELAY, MAX_RETRY_DELAY};
use base::currency::Currency;
use gamblers::{self, BoxedGambler, Message};
use gamblers::Message::*;
use markets::{OID, Offer, Outcome};

use self::Stage::*;

/*                     Aborted
 *                    ↗     ↑
 * Initial → Preparing → Running
 *               ⤡        ↙
 *                Sleeping
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Initial,
    Preparing,
    Running,
    Sleeping(u32),
    Aborted
}

impl From<isize> for Stage {
    fn from(stage: isize) -> Stage {
        match stage {
            -1 => Initial,
            -2 => Preparing,
            -3 => Running,
            -4 => Aborted,
            wakeup if wakeup > 0 => Sleeping(wakeup as u32),
            _ => unreachable!()
        }
    }
}

impl Into<isize> for Stage {
    fn into(self) -> isize {
        match self {
            Initial => -1,
            Preparing => -2,
            Running => -3,
            Aborted => -4,
            Sleeping(wakeup) => wakeup as isize
        }
    }
}

pub struct Bookie {
    pub host: String,
    username: String,
    password: String,
    module: &'static str,
    gambler: BoxedGambler,
    stage: AtomicIsize,
    delay: AtomicUsize,
    balance: AtomicIsize,
    offers: Mutex<HashMap<OID, Offer>>
}

impl PartialEq for Bookie {
    fn eq(&self, other: &Bookie) -> bool {
        self as *const _ == other as *const _
    }
}

impl Bookie {
    pub fn new(host: &str, username: &str, password: &str) -> Bookie {
        let (module, gambler) = gamblers::new(host);

        Bookie {
            host: host.to_owned(),
            username: username.to_owned(),
            password: password.to_owned(),
            module: module,
            gambler: gambler,
            stage: AtomicIsize::new(Initial.into()),
            delay: AtomicUsize::new(0),
            balance: AtomicIsize::new(0),
            offers: Mutex::new(HashMap::new())
        }
    }

    pub fn stage(&self) -> Stage {
        self.stage.load(Relaxed).into()
    }

    fn set_stage(&self, stage: Stage) {
        self.stage.store(stage.into(), Relaxed);
    }

    pub fn balance(&self) -> Currency {
        Currency(self.balance.load(Relaxed) as i64)
    }

    fn set_balance(&self, balance: Currency) {
        self.balance.store(balance.0 as isize, Relaxed);
    }

    fn delay(&self) -> u32 {
        self.delay.load(Relaxed) as u32
    }

    fn set_delay(&self, delay: u32) {
        self.delay.store(delay as usize, Relaxed);
    }

    pub fn hold_stake(&self, stake: Currency) {
        debug_assert_eq!(self.stage(), Running);
        self.balance.fetch_sub(stake.0 as isize, Relaxed);
    }

    pub fn release_stake(&self, stake: Currency) {
        debug_assert_eq!(self.stage(), Running);
        self.balance.fetch_add(stake.0 as isize, Relaxed);
    }

    pub fn drain(&self) -> Vec<Offer> {
        let mut offers = self.offers.lock().unwrap();
        // Workaround rust-lang/rust#21114.
        return offers.drain().map(|(_, o)| o).collect();
    }

    pub fn watch<F: Fn(Offer, bool)>(&self, cb: F) {
        debug_assert!(match self.stage() { Initial | Sleeping(_) => true, _ => false });

        struct Guard<'a>(&'a Bookie);

        impl<'a> Drop for Guard<'a> {
            fn drop(&mut self) {
                if thread::panicking() {
                    self.0.set_stage(Aborted);
                    error!(target: self.0.module, "Aborted due to panic");
                }
            }
        }

        let _guard = Guard(self);

        self.sleep_if_needed();
        self.run(cb);
        self.schedule_sleep();
    }

    pub fn glance_offer(&self, offer: &Offer) -> bool {
        let offers = self.offers.lock().unwrap();

        let is_actual = offers.get(&offer.oid).map_or(false, |stored| {
            // TODO(loyd): change it after #78.
            stored == offer && stored.outcomes == offer.outcomes
        });

        if !is_actual {
            warn!(target: self.module, "Offer {} is suddenly outdated", offer);
        }

        is_actual
    }

    pub fn check_offer(&self, offer: &Offer, outcome: &Outcome, stake: Currency) -> Option<bool> {
        match self.gambler.check_offer(offer, outcome, stake) {
            Ok(true) => Some(true),
            Ok(false) => {
                warn!(target: self.module, "Offer {} is outdated", offer);
                Some(false)
            },
            Err(error) => {
                error!(target: self.module, "While checking offer: {}", error);
                None
            }
        }
    }

    pub fn place_bet(&self, offer: Offer, outcome: Outcome, stake: Currency) -> bool {
        if cfg!(feature = "place-bets") {
            if let Err(error) = self.gambler.place_bet(offer, outcome, stake) {
                error!(target: self.module, "While placing bet: {}", error);
                return false;
            }
        }

        if let Err(error) = self.gambler.check_balance().map(|b| self.set_balance(b)) {
            error!(target: self.module, "While checking balance: {}", error);
            return false;
        }

        true
    }

    fn sleep_if_needed(&self) {
        if let Sleeping(wakeup) = self.stage() {
            let now = time::get_time().sec as u32;

            if now < wakeup {
                let delay = wakeup - now;
                let (hours, mins, secs) = (delay / 3600, delay / 60 % 60, delay % 60);
                info!(target: self.module, "Sleeping for {:02}:{:02}:{:02}", hours, mins, secs);
                thread::sleep(Duration::new((wakeup - now) as u64, 0));
            }
        }
    }

    fn run<F: Fn(Offer, bool)>(&self, cb: F) {
        self.set_stage(Preparing);

        info!(target: self.module, "Authorizating...");

        if let Err(error) = self.gambler.authorize(&self.username, &self.password) {
            error!(target: self.module, "While authorizating: {}", error);
            return;
        }

        info!(target: self.module, "Checking balance...");

        if let Err(error) = self.gambler.check_balance().map(|b| self.set_balance(b)) {
            error!(target: self.module, "While checking balance: {}", error);
            return;
        }

        info!(target: self.module, "Watching for offers...");

        self.set_stage(Running);

        if let Err(error) = self.gambler.watch(&|message| {
            self.set_delay(0);

            // If errors occured at the time of betting.
            if self.stage() != Running {
                panic!("Some error occured while betting");
            }

            self.handle_message(message, &cb);
        }) {
            error!(target: self.module, "While watching: {}", error);
            return;
        }
    }

    fn schedule_sleep(&self) {
        let now = time::get_time().sec as u32;

        let min = MIN_RETRY_DELAY.as_secs() as u32;
        let max = MAX_RETRY_DELAY.as_secs() as u32;

        let delay = cmp::max(min, cmp::min(self.delay() * 2, max));

        self.set_stage(Sleeping(now + delay).into());
        self.set_delay(delay);
    }

    fn handle_message<F: Fn(Offer, bool)>(&self, message: Message, cb: &F) {
        let mut offers = self.offers.lock().unwrap();

        match message {
            Upsert(offer) => {
                if !offers.contains_key(&offer.oid) {
                    cb(offer.clone(), true);
                    offers.insert(offer.oid, offer);
                    return;
                }

                let stored = offers.get_mut(&offer.oid).unwrap();

                // TODO(loyd): change it after #78.
                if stored != &offer {
                    cb(stored.clone(), false);
                    cb(offer.clone(), true);
                    return;
                } else if stored.outcomes != offer.outcomes {
                    cb(offer.clone(), true);
                }

                *stored = offer;
            },
            Remove(oid) => {
                if let Some(offer) = offers.remove(&oid) {
                    cb(offer, false);
                }
            }
        }
    }
}
