use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::mpsc::{self, Sender, Receiver};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use crossbeam;

use base::config::CONFIG;
use base::currency::Currency;
use events::{Offer, Outcome};
use gamblers::{self, BoxedGambler};
use opportunity::{self, Strategy, MarkedOutcome};

pub struct Bookie {
    pub host: String,
    active: AtomicBool,
    balance: AtomicIsize,
    username: String,
    password: String,
    gambler: BoxedGambler
}

impl Bookie {
    pub fn active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    pub fn balance(&self) -> Currency {
        Currency(self.balance.load(Ordering::Relaxed) as i64)
    }

    fn set_active(&self, active: bool) {
         self.active.store(active, Ordering::Relaxed);
    }

    fn set_balance(&self, balance: Currency) {
        self.balance.store(balance.0 as isize, Ordering::Relaxed);
    }
}

pub struct MarkedOffer(pub &'static Bookie, pub Offer);
pub type Event = Vec<MarkedOffer>;
pub type Events = HashMap<Offer, Event>;

lazy_static! {
    pub static ref BOOKIES: Vec<Bookie> = init_bookies();
    static ref EVENTS: RwLock<Events> = RwLock::new(HashMap::new());
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

        bookies.push(Bookie {
            host: host.to_owned(),
            active: AtomicBool::new(false),
            balance: AtomicIsize::new(0),
            username: username.to_owned(),
            password: password.to_owned(),
            gambler: gamblers::new(host)
        });
    }

    bookies
}

fn run_gambler(bookie: &'static Bookie,
               incoming: Sender<MarkedOffer>,
               outgoing: Sender<MarkedOffer>) {
    // TODO(loyd): add error handling (don't forget about catching panics!).
    if let Err(error) = bookie.gambler.authorize(&bookie.username, &bookie.password) {
        error!("{} authorization: {}", bookie.host, error);
        return;
    }

    let balance = match bookie.gambler.check_balance() {
        Ok(balance) => balance,
        Err(error) => {
            error!("{} balance checking: {}", bookie.host, error);
            return;
        }
    };

    bookie.set_balance(balance);
    bookie.set_active(true);

    let error = bookie.gambler.watch(&|offer, update| {
        let marked = MarkedOffer(bookie, offer);

        if update {
            incoming.send(marked).unwrap();
        } else {
            outgoing.send(marked).unwrap();
        }
    }).unwrap_err();

    error!("{}: {}", bookie.host, error);

    bookie.set_active(false);
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
        let index = event.iter()
            .position(|stored| stored.0 as *const _ == marked.0 as *const _);

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

        let index = event.iter()
            .position(|stored| stored.0 as *const _ == marked.0 as *const _);

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

    let mut table = Vec::with_capacity(event.len());

    for marked in event {
        // We assume that sorting by coefs is reliable way to collate outcomes.
        let marked = sort_outcomes_by_coef(&marked.1.outcomes);
        table.push(marked);
    }

    debug!("Checking event:");

    for &MarkedOffer(bookie, ref offer) in event {
        debug!("    {} by {}", offer, bookie.host);
    }

    let margin = opportunity::calc_margin(&table);

    if margin < 1. {
        let outcomes = opportunity::find_best(&table, Strategy::Unbiased);

        info!("  Opportunity exists (effective margin: {:.2}), unbiased strategy:", margin);

        for MarkedOutcome { market, outcome, rate, profit } in outcomes {
            let host = &event[market].0.host;
            info!("    Place {:.2} on {} by {} (coef: x{:.2}, profit: {:+.1}%)",
                  rate, outcome.0, host, outcome.1, profit * 100.);
        }
    } else {
        debug!("  Opportunity doesn't exist (effective margin: {:.2})", margin);
    }
}

fn sort_outcomes_by_coef(outcomes: &[Outcome]) -> Vec<&Outcome> {
    let mut result = outcomes.iter().collect::<Vec<_>>();
    result.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    result
}
