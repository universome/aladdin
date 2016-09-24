use std::thread;
use std::cmp::Ordering;
use std::time::Duration;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicIsize};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::mpsc::{self, Sender, Receiver};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use crossbeam;
use time;

use base::config::CONFIG;
use base::currency::Currency;
use events::{Offer, Outcome, DRAW, fuzzy_eq};
use gamblers::{self, BoxedGambler};
use opportunity::{self, Strategy, MarkedOutcome};
use combo::{self, Combo, Bet};

pub struct Bookie {
    pub host: String,
    active: AtomicBool,
    balance: AtomicIsize,
    username: String,
    password: String,
    module: &'static str,
    gambler: BoxedGambler
}

impl PartialEq for Bookie {
    fn eq(&self, other: &Bookie) -> bool {
        self as *const _ == other as *const _
    }
}

impl Bookie {
    pub fn active(&self) -> bool {
        self.active.load(Relaxed)
    }

    pub fn balance(&self) -> Currency {
        Currency(self.balance.load(Relaxed) as i64)
    }

    fn activate(&self) {
        self.active.store(true, Relaxed);
    }

    fn deactivate(&self) {
        self.active.store(false, Relaxed);
    }

    fn set_balance(&self, balance: Currency) {
        self.balance.store(balance.0 as isize, Relaxed);
    }
}

#[derive(Clone)]
pub struct MarkedOffer(pub &'static Bookie, pub Offer);
pub type Event = Vec<MarkedOffer>;
pub type Events = HashMap<Offer, Event>;

lazy_static! {
    pub static ref BOOKIES: Vec<Bookie> = init_bookies();
    static ref EVENTS: RwLock<Events> = RwLock::new(HashMap::new());

    // TODO(loyd): add getters to `config` module and refactor this.
    static ref BET_SIZE: Currency = CONFIG.lookup("arbitrer.bet-size")
        .unwrap().as_float().unwrap().into();
    static ref COMBO_HISTORY_SIZE: u32 = CONFIG.lookup("arbitrer.history-size")
        .unwrap().as_integer().unwrap() as u32 * 3600;
    static ref LOWER_PROFIT_THRESHOLD: f64 = CONFIG.lookup("arbitrer.lower-profit-threshold")
        .unwrap().as_float().unwrap();
    static ref UPPER_PROFIT_THRESHOLD: f64 = CONFIG.lookup("arbitrer.upper-profit-threshold")
        .unwrap().as_float().unwrap();
}

pub fn acquire_events() -> RwLockReadGuard<'static, Events> {
    EVENTS.read().unwrap()
}

fn acquire_events_mut() -> RwLockWriteGuard<'static, Events> {
    EVENTS.write().unwrap()
}

pub fn run() {
    let (incoming_tx, incoming_rx) = mpsc::channel();
    let (outgoing_tx, outgoing_rx) = mpsc::channel();

    crossbeam::scope(|scope| {
        for bookie in BOOKIES.iter() {
            let incoming_tx = incoming_tx.clone();
            let outgoing_tx = outgoing_tx.clone();

            scope.spawn(move || run_gambler(bookie, incoming_tx, outgoing_tx));
        }

        process_channels(incoming_rx, outgoing_rx);
    });
}

fn init_bookies() -> Vec<Bookie> {
    let mut bookies = Vec::new();
    let array = CONFIG.lookup("bookies").unwrap().as_slice().unwrap();

    for item in array {
        let enabled = item.lookup("enabled").map_or(true, |x| x.as_bool().unwrap());

        if !enabled {
            continue;
        }

        let host = item.lookup("host").unwrap().as_str().unwrap();
        let username = item.lookup("username").unwrap().as_str().unwrap();
        let password = item.lookup("password").unwrap().as_str().unwrap();
        let (module, gambler) = gamblers::new(host);

        bookies.push(Bookie {
            host: host.to_owned(),
            active: AtomicBool::new(false),
            balance: AtomicIsize::new(0),
            username: username.to_owned(),
            password: password.to_owned(),
            module: module,
            gambler: gambler
        });
    }

    bookies
}

fn run_gambler(bookie: &'static Bookie,
               incoming: Sender<MarkedOffer>,
               outgoing: Sender<MarkedOffer>)
{
    struct Guard(&'static Bookie);

    impl Drop for Guard {
        fn drop(&mut self) {
            regression(self.0);

            if thread::panicking() {
                error!(target: self.0.module, "Terminated due to panic");
            }
        }
    }

    let module = bookie.module;
    let retry_delay = CONFIG.lookup("arbitrer.retry-delay")
        .and_then(|d| d.as_integer())
        .map(|d| 60 * d as u64)
        .unwrap();

    let mut delay = 0;

    loop {
        if delay > 0 {
            info!(target: module, "Sleeping for {:02}:{:02}", delay / 60, delay % 60);
            thread::sleep(Duration::new(delay, 0));
            delay *= 2;
        } else {
            delay = retry_delay;
        }

        let _guard = Guard(bookie);

        info!(target: module, "Authorizating...");

        if let Err(error) = bookie.gambler.authorize(&bookie.username, &bookie.password) {
            error!(target: module, "While authorizating: {}", error);
            continue;
        }

        info!(target: module, "Checking balance...");

        if let Err(error) = bookie.gambler.check_balance().map(|b| bookie.set_balance(b)) {
            error!(target: module, "While checking balance: {}", error);
            continue;
        }

        info!(target: module, "Watching for offers...");
        bookie.activate();

        if let Err(error) = bookie.gambler.watch(&|offer, update| {
            // If errors occured at the time of betting.
            if !bookie.active() {
                panic!("Some error occured while betting");
            }

            let marked = MarkedOffer(bookie, offer);
            let chan = if update { &incoming } else { &outgoing };
            chan.send(marked).unwrap();
        }) {
            error!(target: module, "While watching: {}", error);
            continue;
        }

        unreachable!();
    }
}

fn regression(bookie: &Bookie) {
    let mut events = acquire_events_mut();

    bookie.deactivate();

    let outdated = events.values()
        .flat_map(|offers| offers.iter().filter(|o| o.0 == bookie))
        .cloned()
        .collect::<Vec<_>>();

    info!("Regression of {}. Removing {} offers...", bookie.host, outdated.len());

    for marked in outdated {
        remove_offer(&mut events, marked);
    }
}

fn process_channels(incoming: Receiver<MarkedOffer>, outgoing: Receiver<MarkedOffer>) {
    loop {
        let marked = incoming.recv().unwrap();
        let key = marked.1.clone();
        let mut events = acquire_events_mut();

        update_offer(&mut events, marked);

        while let Ok(marked) = outgoing.try_recv() {
            remove_offer(&mut events, marked);
        }

        if let Some(event) = events.get(&key) {
            realize_event(event);
        }
    }
}

fn remove_offer(events: &mut Events, marked: MarkedOffer) {
    let mut remove_event = false;

    if let Some(event) = events.get_mut(&marked.1) {
        let index = event.iter().position(|stored| stored.0 == marked.0);

        if let Some(index) = index {
            event.swap_remove(index);
            debug!("{} by {} is removed", marked.1, marked.0.host);
        } else {
            warn!("There is no {} by {}", marked.1, marked.0.host);
        }

        remove_event = event.is_empty();
    }

    if remove_event {
        debug!("Event [{} by {}] is removed", marked.1, marked.0.host);
        events.remove(&marked.1);
    }
}

fn update_offer(events: &mut Events, marked: MarkedOffer) {
    if events.contains_key(&marked.1) {
        let event = events.get_mut(&marked.1).unwrap();
        let index = event.iter().position(|stored| stored.0 == marked.0);

        if let Some(index) = index {
            if marked.1.outcomes.len() != event[index].1.outcomes.len() {
                error!("{} by {} is NOT updated: incorrect dimension", marked.1, marked.0.host);
                return;
            }

            debug!("{} by {} is updated", marked.1, marked.0.host);
            event[index] = marked;
        } else {
            if marked.1.outcomes.len() != event[0].1.outcomes.len() {
                error!("{} by {} is NOT added: incorrect dimension", marked.1, marked.0.host);
                return;
            }

            debug!("{} by {} is added", marked.1, marked.0.host);
            event.push(marked);
        }
    } else {
        debug!("Event [{} by {}] is added", marked.1, marked.0.host);
        events.insert(marked.1.clone(), vec![marked]);
    }
}

fn realize_event(event: &Event) {
    if event.len() < 2 {
        return;
    }

    let mut table: Vec<Vec<_>> = Vec::with_capacity(event.len());

    for (i, marked) in event.into_iter().enumerate() {
        // We assume that sorting by coefs is reliable way to collate outcomes.
        let mut marked = sort_outcomes_by_coef(&marked.1.outcomes);

        if i > 0 {
            comparative_permutation(&mut marked, &table[0]);
        }

        table.push(marked);
    }

    debug!("Checking event:");

    for &MarkedOffer(bookie, ref offer) in event {
        debug!("    {} by {}", offer, bookie.host);
    }

    let margin = opportunity::calc_margin(&table);

    if margin < 1. {
        let outcomes = opportunity::find_best(&table, Strategy::Unbiased);
        let mut min_profit = 1. / 0.;
        let mut max_profit = 0.;

        info!("  Opportunity exists (effective margin: {:.2}), unbiased strategy:", margin);

        for &MarkedOutcome { market, outcome, rate, profit } in &outcomes {
            let host = &event[market].0.host;

            info!("    Place {:.2} on {} by {} (coef: x{:.2}, profit: {:+.1}%)",
                  rate, outcome.0, host, outcome.1, profit * 100.);

            if profit < min_profit { min_profit = profit }
            if profit > max_profit { max_profit = profit }
        }

        if *LOWER_PROFIT_THRESHOLD <= min_profit && min_profit <= *UPPER_PROFIT_THRESHOLD {
            // TODO(loyd): drop offers instead of whole events.
            if no_bets_on_event(event) {
                save_combo(event, &outcomes);
                place_bet(event, &outcomes);
            }
        } else if max_profit > *UPPER_PROFIT_THRESHOLD {
            warn!("Suspiciously high profit ({:+.1}%)", max_profit * 100.);
        } else {
             debug!("  Too small profit (min: {:+.1}%, max: {:+.1}%)",
                    min_profit * 100., max_profit * 100.);
        }
    } else {
        debug!("  Opportunity doesn't exist (effective margin: {:.2})", margin);
    }
}

fn sort_outcomes_by_coef(outcomes: &[Outcome]) -> Vec<&Outcome> {
    let mut result = outcomes.iter().collect::<Vec<_>>();

    result.sort_by(|a, b| {
       if a.0 == DRAW {
           Ordering::Greater
       } else if b.0 == DRAW {
           Ordering::Less
       } else {
           a.1.partial_cmp(&b.1).unwrap()
       }
    });

    result
}

fn comparative_permutation(outcomes: &mut [&Outcome], ideal: &[&Outcome]) {
    for i in 0..ideal.len() - 1 {
        if fuzzy_eq(&ideal[i].0, &outcomes[i+1].0) || fuzzy_eq(&outcomes[i].0, &ideal[i+1].0) {
            outcomes.swap(i, i + 1);
        }
    }
}

fn no_bets_on_event(event: &Event) -> bool {
    // TODO(loyd): what about bulk checking?
    event.iter().any(|marked| combo::contains(&marked.0.host, marked.1.inner_id))
}

fn save_combo(event: &Event, outcomes: &[MarkedOutcome]) {
    debug!("New combo for {}", event[0].1);

    let now = time::get_time().sec as u32;

    combo::save(Combo {
        date: now,
        kind: format!("{:?}", event[0].1.kind),
        bets: outcomes.iter().map(|m| Bet {
            host: event[m.market].0.host.clone(),
            id: event[m.market].1.inner_id,
            title: if m.outcome.0 == DRAW { None } else { Some(m.outcome.0.clone()) },
            expiry: event[m.market].1.date,
            coef: m.outcome.1,
            size: m.rate * *BET_SIZE,
            profit: m.profit
        }).collect()
    });
}

#[cfg(feature = "place-bets")]
fn place_bet(event: &Event, outcomes: &[MarkedOutcome]) {
    struct Guard(&'static Bookie, bool);

    impl Drop for Guard {
        fn drop(&mut self) {
            if !self.1 {
                regression(self.0);
            }
        }
    }

    for marked in outcomes {
        let bookie = event[marked.market].0;
        let offer = event[marked.market].1.clone();
        let outcome = marked.outcome.clone();
        let bet_size = marked.rate * *BET_SIZE;

        thread::spawn(move || {
            let mut guard = Guard(bookie, false);

            if let Err(error) = bookie.gambler.place_bet(offer, outcome, bet_size) {
                error!(target: bookie.module, "While placing bet: {}", error);
                return;
            }

            if let Err(error) = bookie.gambler.check_balance().map(|b| bookie.set_balance(b)) {
                error!(target: bookie.module, "While checking balance: {}", error);
                return;
            }

            guard.1 = true;
        });
    }
}

#[cfg(not(feature = "place-bets"))]
fn place_bet(_: &Event, _: &[MarkedOutcome]) {}
