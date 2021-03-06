use std::cmp;
use std::thread;
use std::time::Duration;
use std::sync::atomic::{AtomicIsize, AtomicUsize};
use std::sync::atomic::Ordering::Relaxed;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use parking_lot::RwLock;
use time;

use constants::{MIN_RETRY_DELAY, MAX_RETRY_DELAY};
use base::currency::Currency;
use arbitrer::matcher;
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
    #[inline]
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
    #[inline]
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
    offers: RwLock<HashMap<OID, Offer>>
}

impl PartialEq for Bookie {
    #[inline]
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
            offers: RwLock::new(HashMap::new())
        }
    }

    #[inline]
    pub fn stage(&self) -> Stage {
        self.stage.load(Relaxed).into()
    }

    #[inline]
    fn set_stage(&self, stage: Stage) {
        self.stage.store(stage.into(), Relaxed);
    }

    #[inline]
    pub fn balance(&self) -> Currency {
        Currency(self.balance.load(Relaxed) as i64)
    }

    #[inline]
    fn set_balance(&self, balance: Currency) {
        self.balance.store(balance.0 as isize, Relaxed);
    }

    #[inline]
    fn delay(&self) -> u32 {
        self.delay.load(Relaxed) as u32
    }

    #[inline]
    fn set_delay(&self, delay: u32) {
        self.delay.store(delay as usize, Relaxed);
    }

    #[inline]
    pub fn offer_count(&self) -> usize {
        self.offers.read().len()
    }

    #[inline]
    pub fn hold_stake(&self, stake: Currency) {
        self.balance.fetch_sub(stake.0 as isize, Relaxed);
    }

    #[inline]
    pub fn release_stake(&self, stake: Currency) {
        self.balance.fetch_add(stake.0 as isize, Relaxed);
    }

    pub fn drain(&self) -> Vec<Offer> {
        let mut offers = self.offers.write();
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
        let offers = self.offers.read();
        offers.get(&offer.oid).map_or(false, |o| o == offer)
    }

    pub fn check_offer(&self, offer: &Offer, outcome: &Outcome, stake: Currency) -> Option<bool> {
        match self.gambler.check_offer(offer, outcome, stake) {
            Ok(true) => Some(true),
            Ok(false) => {
                warn!(target: self.module, "Offer {} is outdated", offer);
                Some(false)
            },
            Err(error) => {
                error!(target: self.module, "While checking offer: {}\n{:?}", error, error.stack);
                None
            }
        }
    }

    pub fn place_bet(&self, offer: Offer, outcome: Outcome, stake: Currency) -> bool {
        if cfg!(feature = "place-bets") {
            if let Err(error) = self.gambler.place_bet(offer, outcome, stake) {
                error!(target: self.module, "While placing bet: {}\n{:?}", error, error.stack);
                return false;
            }
        }

        if let Err(error) = self.gambler.check_balance().map(|b| self.set_balance(b)) {
            error!(target: self.module, "While checking balance: {}\n{:?}", error, error.stack);
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
            error!(target: self.module, "While authorizating: {}\n{:?}", error, error.stack);
            return;
        }

        info!(target: self.module, "Checking balance...");

        if let Err(error) = self.gambler.check_balance().map(|b| self.set_balance(b)) {
            error!(target: self.module, "While checking balance: {}\n{:?}", error, error.stack);
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
            error!(target: self.module, "While watching: {}\n{:?}", error, error.stack);
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
        let mut offers = self.offers.write();

        let (remove, upsert) = match message {
            Upsert(offer) => match offers.entry(offer.oid) {
                Entry::Vacant(entry) => {
                    entry.insert(offer.clone());
                    (None, Some(offer))
                },
                Entry::Occupied(ref entry) if entry.get() == &offer => return,
                Entry::Occupied(mut entry) => {
                    if matcher::get_headline(&offer) == matcher::get_headline(entry.get()) {
                        *entry.get_mut() = offer.clone();
                        (None, Some(offer))
                    } else {
                        let stored = entry.insert(offer.clone());
                        (Some(stored), Some(offer))
                    }
                }
            },
            Remove(oid) => (offers.remove(&oid), None)
        };

        // Drop the guard before calling the callback to prevent possible deadlocks.
        drop(offers);

        // We should remove before upsert for readding cases.
        if let Some(remove) = remove {
            cb(remove, false);
        }

        if let Some(upsert) = upsert {
            cb(upsert, true);
        }
    }
}
