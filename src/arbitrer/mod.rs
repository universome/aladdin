use std::thread;
use std::cmp::Ordering;
use std::time::Instant;
use std::sync::mpsc::{self, Sender, Receiver};
use std::sync::{Arc, Mutex, RwLock, Condvar, RwLockReadGuard};
use time;

use constants::{RETRY_DELAY, CHECK_TIMEOUT, BASE_STAKE, MAX_STAKE, MIN_PROFIT, MAX_PROFIT};
use constants::BOOKIES_AUTH;
use base::currency::Currency;
use events::{Offer, Outcome, DRAW, fuzzy_eq};
use combo::{self, Combo, Bet};

pub use self::bookie::{Bookie, MarkedOffer};
pub use self::bucket::Bucket;

use self::opportunity::{Strategy, MarkedOutcome};

mod bookie;
mod bucket;
mod opportunity;

lazy_static! {
    pub static ref BOOKIES: Vec<Bookie> = init_bookies();
    static ref BUCKET: RwLock<Bucket> = RwLock::new(Bucket::new());
}

pub fn acquire_bucket() -> RwLockReadGuard<'static, Bucket> {
    BUCKET.read().unwrap()
}

pub fn run() {
    let (incoming_tx, incoming_rx) = mpsc::channel();
    let (outgoing_tx, outgoing_rx) = mpsc::channel();

    for bookie in BOOKIES.iter() {
        let incoming_tx = incoming_tx.clone();
        let outgoing_tx = outgoing_tx.clone();

        thread::spawn(move || run_gambler(bookie, incoming_tx, outgoing_tx));
    }

    process_channels(incoming_rx, outgoing_rx);
}

fn init_bookies() -> Vec<Bookie> {
    BOOKIES_AUTH.iter().map(|info| Bookie::new(info.0, info.1, info.2)).collect()
}

fn run_gambler(bookie: &'static Bookie,
               incoming: Sender<MarkedOffer>,
               outgoing: Sender<MarkedOffer>)
{
    struct Guard(&'static Bookie);

    impl Drop for Guard {
        fn drop(&mut self) {
            degradation(self.0);

            if thread::panicking() {
                error!(target: self.0.module, "Terminated due to panic");
            }
        }
    }

    let module = bookie.module;
    let mut delay = *RETRY_DELAY;
    let mut started = false;

    loop {
        if started {
            let secs = delay.as_secs();
            info!(target: module, "Sleeping for {:02}:{:02}", secs / 60, secs % 60);
            thread::sleep(delay);
            delay *= 2;
        } else {
            started = true;
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

        delay = *RETRY_DELAY;

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

fn degradation(bookie: &Bookie) {
    if !bookie.deactivate() {
        return;
    }

    let mut bucket = BUCKET.write().unwrap();
    bucket.remove_offers_by_bookie(bookie);
}

fn process_channels(incoming: Receiver<MarkedOffer>, outgoing: Receiver<MarkedOffer>) {
    loop {
        let marked = incoming.recv().unwrap();
        let key = marked.1.clone();
        let mut bucket = BUCKET.write().unwrap();

        bucket.update_offer(marked);

        while let Ok(marked) = outgoing.try_recv() {
            bucket.remove_offer(&marked);
        }

        if let Some(market) = bucket.get_market(&key) {
            realize_market(market);
        }
    }
}

fn realize_market(market: &[MarkedOffer]) {
    if market.len() < 2 {
        return;
    }

    let mut table: Vec<Vec<_>> = Vec::with_capacity(market.len());

    for (i, marked) in market.into_iter().enumerate() {
        // We assume that sorting by coefs is reliable way to collate outcomes.
        let mut marked = sort_outcomes_by_coef(&marked.1.outcomes);

        if i > 0 {
            comparative_permutation(&mut marked, &table[0]);
        }

        table.push(marked);
    }

    debug!("Checking market:");

    for &MarkedOffer(bookie, ref offer) in market {
        debug!("    {} by {}", offer, bookie.host);
    }

    let margin = opportunity::calc_margin(&table);

    if margin >= 1. {
        debug!("  Opportunity doesn't exist (effective margin: {:.2})", margin);
        return;
    }

    let outcomes = opportunity::find_best(&table, Strategy::Unbiased);
    let mut min_profit = 1. / 0.;
    let mut max_profit = 0.;

    info!("  Opportunity exists (effective margin: {:.2}), unbiased strategy:", margin);

    for &MarkedOutcome { market: m, outcome, rate, profit } in &outcomes {
        let host = &market[m].0.host;

        info!("    Place {:.2} on {} by {} (coef: x{:.2}, profit: {:+.1}%)",
              rate, outcome.0, host, outcome.1, profit * 100.);

        if profit < min_profit { min_profit = profit }
        if profit > max_profit { max_profit = profit }
    }

    if MIN_PROFIT <= min_profit && min_profit <= MAX_PROFIT {
        // TODO(loyd): drop offers instead of whole bucket.
        if !no_bets_on_market(market) {
            return;
        }

        let pairs = outcomes.iter().map(|o| (&market[o.market], o)).collect::<Vec<_>>();

        let stakes = match distribute_currency(&pairs) {
            Some(stakes) => stakes,
            None => return
        };

        place_bets(&pairs, &stakes);
    } else if max_profit > MAX_PROFIT {
        warn!("Suspiciously high profit ({:+.1}%)", max_profit * 100.);
    } else {
        debug!("  Too small profit (min: {:+.1}%, max: {:+.1}%)",
               min_profit * 100., max_profit * 100.);
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

fn no_bets_on_market(market: &[MarkedOffer]) -> bool {
    // TODO(loyd): what about bulk checking?
    !market.iter().any(|marked| combo::contains(&marked.0.host, marked.1.inner_id))
}

fn distribute_currency(pairs: &[(&MarkedOffer, &MarkedOutcome)]) -> Option<Vec<Currency>> {
    let mut base_rate = pairs[0].1.rate;

    for &(_, marked_outcome) in pairs {
        if marked_outcome.rate < base_rate { base_rate = marked_outcome.rate }
    }

    let mut stakes = Vec::with_capacity(pairs.len());

    for &(marked_offer, marked_outcome) in pairs {
        let bookie = marked_offer.0;
        let stake = marked_outcome.rate / base_rate * *BASE_STAKE;

        if stake > *MAX_STAKE {
            warn!("Too high stake ({})", stake);
            return None;
        }

        let balance = bookie.balance();

        if stake > balance {
            warn!("Not enough money on {} ({}, but required {})", bookie.host, balance, stake);
            return None;
        }

        stakes.push(stake);
    }

    for (&(marked, _), &stake) in pairs.iter().zip(stakes.iter()) {
        marked.0.hold_stake(stake);
    }

    Some(stakes)
}

fn save_combo(pairs: &[(&MarkedOffer, &MarkedOutcome)], stakes: &[Currency]) {
    debug_assert_eq!(pairs.len(), stakes.len());

    combo::save(Combo {
        date: time::get_time().sec as u32,
        kind: format!("{:?}", (pairs[0].0).1.kind),
        bets: pairs.iter().zip(stakes.iter()).map(|(&(m, o), stake)| Bet {
            host: m.0.host.clone(),
            id: m.1.inner_id,
            title: if o.outcome.0 == DRAW { None } else { Some(o.outcome.0.clone()) },
            expiry: m.1.date,
            coef: o.outcome.1,
            stake: *stake,
            profit: o.profit,
            placed: false
        }).collect()
    });
}

fn place_bets(pairs: &[(&MarkedOffer, &MarkedOutcome)], stakes: &[Currency]) {
    debug_assert_eq!(pairs.len(), stakes.len());

    // We cannot use `std::sync::Barrier` because it has small possibility for error handling.
    let barrier = Arc::new((Mutex::new(pairs.len() as u32), Condvar::new()));

    for (&(marked_offer, marked_outcome), &stake) in pairs.iter().zip(stakes.iter()) {
        let bookie = marked_offer.0;
        let offer = marked_offer.1.clone();
        let outcome = marked_outcome.outcome.clone();
        let barrier = barrier.clone();

        thread::spawn(move || {
            let &(ref count, ref cvar) = &*barrier;
            place_bet(bookie, offer, outcome, stake, count, cvar);
        });
    }

    // TODO(loyd): temporary solution, it blocks system during offer checking to prmarket race.
    let mut count = barrier.0.lock().unwrap();
    let start = Instant::now();

    while *count > 0 {
        let result = barrier.1.wait_timeout(count, *CHECK_TIMEOUT).unwrap();

        if result.1.timed_out() || start.elapsed() >= *CHECK_TIMEOUT {
            warn!("The time is up");
            return;
        }

        count = result.0;
    }

    save_combo(&pairs, &stakes);
}

fn place_bet(bookie: &Bookie, offer: Offer, outcome: Outcome, stake: Currency,
             count: &Mutex<u32>, cvar: &Condvar)
{
    struct Guard<'b> {
        bookie: &'b Bookie,
        hold: Option<Currency>,
        done: bool
    }

    impl<'b> Drop for Guard<'b> {
        fn drop(&mut self) {
            if !self.done {
                degradation(self.bookie);
            }

            if let Some(stake) = self.hold {
                self.bookie.release_stake(stake);
            }
        }
    }

    let mut guard = Guard {
        bookie: bookie,
        hold: Some(stake),
        done: false
    };

    match bookie.gambler.check_offer(&offer, &outcome, stake) {
        Ok(true) => {},
        Ok(false) => {
            warn!(target: bookie.module, "Offer {} is outdated", offer);
            guard.done = true;
            return;
        },
        Err(error) => {
            error!(target: bookie.module, "While checking offer: {}", error);
            return;
        }
    }

    let mut count = count.lock().unwrap();
    *count -= 1;

    if *count > 0 {
        // Wait for the end of the check of other offers.
        while *count > 0 {
            let result = cvar.wait_timeout(count, *CHECK_TIMEOUT).unwrap();
            count = result.0;

            // Either the time is up or some thread fails.
            if result.1.timed_out() {
                guard.done = true;
                return;
            }
        }
    } else {
        cvar.notify_all();
    }

    drop(count);

    let inner_id = offer.inner_id;

    if cfg!(feature = "place-bets") {
        if let Err(error) = bookie.gambler.place_bet(offer, outcome, stake) {
            error!(target: bookie.module, "While placing bet: {}", error);
            return;
        }

        guard.hold = None;
    }

    combo::mark_as_placed(&bookie.host, inner_id);

    if let Err(error) = bookie.gambler.check_balance().map(|b| bookie.set_balance(b)) {
        error!(target: bookie.module, "While checking balance: {}", error);
        return;
    }

    guard.done = true;
}
