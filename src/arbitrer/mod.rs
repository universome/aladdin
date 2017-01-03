use std::thread;
use std::sync::mpsc::{self, Sender, Receiver};
use std::sync::Arc;
use time;

use constants::{TABLE_CAPACITY, CHECK_TIMEOUT, BASE_STAKE, MAX_STAKE, MIN_PROFIT, MAX_PROFIT};
use constants::ACCOUNTS;
use base::currency::Currency;
use base::barrier::Barrier;
use markets::{Offer, Outcome, DRAW};
use combo::{self, Combo, Bet};

pub use self::bookie::Bookie;
pub use self::bookie::Stage as BookieStage;
pub use self::table::Table;

use self::opportunity::{Strategy, MarkedOutcome};

#[derive(Clone)]
pub struct MarkedOffer(pub &'static Bookie, pub Offer);

mod matcher;
mod bookie;
mod table;
mod opportunity;

lazy_static! {
    pub static ref BOOKIES: Vec<Bookie> = init_bookies();
    pub static ref TABLE: Table = Table::new(TABLE_CAPACITY);
}

pub fn run() {
    let (tx, rx) = mpsc::channel();

    accumulation(tx);
    resolution(rx);
}

fn init_bookies() -> Vec<Bookie> {
    ACCOUNTS.iter().map(|info| Bookie::new(info.0, info.1, info.2)).collect()
}

fn accumulation(chan: Sender<Offer>) {
    for bookie in BOOKIES.iter() {
        let tx = chan.clone();

        thread::Builder::new()
            .name(bookie.host.clone())
            .spawn(move || run_gambler(bookie, tx))
            .unwrap();
    }
}

fn run_gambler(bookie: &'static Bookie, chan: Sender<Offer>) {
    struct Guard(&'static Bookie);

    impl Drop for Guard {
        fn drop(&mut self) {
            degradation(self.0);
        }
    }

    loop {
        let _guard = Guard(bookie);

        bookie.watch(|offer, upsert| {
            let marked = MarkedOffer(bookie, offer.clone());

            if upsert {
                if TABLE.update_offer(marked) >= 2 {
                    chan.send(offer).unwrap();
                }
            } else {
                TABLE.remove_offer(&marked);
            }
        });
    }
}

fn degradation(bookie: &'static Bookie) {
    let outdated = bookie.drain();

    info!("Degradation of {}. Removing {} offers...", bookie.host, outdated.len());

    for offer in outdated {
        TABLE.remove_offer(&MarkedOffer(bookie, offer));
    }
}

fn resolution(chan: Receiver<Offer>) {
    for offer in chan {
        if let Some(market) = TABLE.get_market(&offer) {
            realize_market(&*market);
        }
    }

    info!("Channel has hung up!");
}

fn realize_market(market: &[MarkedOffer]) {
    if market.len() < 2 {
        return;
    }

    if let Some(marked) = market.iter().find(|m| m.0.stage() != BookieStage::Running) {
        warn!("Bookie {} isn't running, but the table contains offer(s) by it", marked.0.host);
        return;
    }

    let mut table: Vec<Vec<_>> = Vec::with_capacity(market.len());
    let etalon = &market[0].1.outcomes;

    table.push(etalon.iter().collect());

    for marked in &market[1..] {
        table.push(matcher::collate_outcomes(etalon, &marked.1.outcomes));
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

    info!("  Opportunity exists [{:?}] {:?} (effective margin: {:.2}), unbiased strategy:",
          (market[0].1).game, (market[0].1).kind, margin);

    for &MarkedOutcome { market: m, outcome, rate, profit } in &outcomes {
        let host = &market[m].0.host;

        info!("    Place {:.2} on {} by {} (coef: x{:.2}, profit: {:+.1}%)",
              rate, outcome.0, host, outcome.1, profit * 100.);

        if profit < min_profit { min_profit = profit }
        if profit > max_profit { max_profit = profit }
    }

    if MIN_PROFIT <= min_profit && min_profit <= MAX_PROFIT {
        // TODO(loyd): drop offers instead of whole market.
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

fn no_bets_on_market(market: &[MarkedOffer]) -> bool {
    // TODO(loyd): what about bulk checking?
    !market.iter().any(|marked| combo::contains(&marked.0.host, marked.1.oid))
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
        game: format!("{:?}", (pairs[0].0).1.game),
        kind: format!("{:?}", (pairs[0].0).1.kind),
        bets: pairs.iter().zip(stakes.iter()).map(|(&(m, o), stake)| Bet {
            host: m.0.host.clone(),
            id: m.1.oid,
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

    let barrier = Arc::new(Barrier::new(pairs.len() as u32 + 1));

    for (&(marked_offer, marked_outcome), &stake) in pairs.iter().zip(stakes.iter()) {
        let bookie = marked_offer.0;
        let offer = marked_offer.1.clone();
        let outcome = marked_outcome.outcome.clone();
        let barrier = barrier.clone();

        thread::spawn(move || {
            place_bet(bookie, offer, outcome, stake, &*barrier);
        });
    }

    // Yes, twice: `glance + check` and `glance`.
    if !barrier.wait_timeout(*CHECK_TIMEOUT) || !barrier.wait_timeout(*CHECK_TIMEOUT) {
        warn!("The time is up");
        return;
    }

    save_combo(&pairs, &stakes);

    // Feuer Frei!
    barrier.wait();
}

fn place_bet(bookie: &'static Bookie, offer: Offer, outcome: Outcome, stake: Currency,
             barrier: &Barrier)
{
    struct Guard {
        bookie: &'static Bookie,
        hold: Option<Currency>,
        done: bool
    }

    impl Drop for Guard {
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

    if !bookie.glance_offer(&offer) {
        error!("Ooops, one of the offers is rotten before the check!");
        guard.done = true;
        return;
    }

    match bookie.check_offer(&offer, &outcome, stake) {
        Some(true) => {},
        Some(false) => {
            guard.done = true;
            return;
        },
        None => return
    }

    // Either the time is up or some thread fails.
    if !barrier.wait_timeout(*CHECK_TIMEOUT) {
        guard.done = true;
        return;
    }

    if !bookie.glance_offer(&offer) {
        error!("Ooops, one of the offers is rotten after the check!");
        guard.done = true;
        return;
    }

    // Some thread fails.
    if !barrier.wait_timeout(*CHECK_TIMEOUT) {
        guard.done = true;
        return;
    }

    // Wait the combo saving.
    barrier.wait();

    let oid = offer.oid;
    let title = outcome.0.clone();
    let opt_title = if title == DRAW { None } else { Some(title.as_str()) };

    if !bookie.place_bet(offer, outcome, stake) {
        return;
    }

    guard.hold = None;
    guard.done = true;

    combo::mark_as_placed(&bookie.host, oid, opt_title);
}
