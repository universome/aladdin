use std::sync::atomic::{AtomicBool, AtomicIsize};
use std::sync::atomic::Ordering::Relaxed;

use base::currency::Currency;
use gamblers::{self, BoxedGambler};
use events::Offer;

#[derive(Clone)]
pub struct MarkedOffer(pub &'static Bookie, pub Offer);

pub struct Bookie {
    pub host: String,
    active: AtomicBool,
    balance: AtomicIsize,
    pub username: String,
    pub password: String,
    pub module: &'static str,
    pub gambler: BoxedGambler
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
            active: AtomicBool::new(false),
            balance: AtomicIsize::new(0),
            username: username.to_owned(),
            password: password.to_owned(),
            module: module,
            gambler: gambler
        }
    }

    pub fn active(&self) -> bool {
        self.active.load(Relaxed)
    }

    pub fn balance(&self) -> Currency {
        Currency(self.balance.load(Relaxed) as i64)
    }

    pub fn activate(&self) -> bool {
        self.active.swap(true, Relaxed)
    }

    pub fn deactivate(&self) -> bool {
        self.active.swap(false, Relaxed)
    }

    pub fn set_balance(&self, balance: Currency) {
        self.balance.store(balance.0 as isize, Relaxed);
    }

    pub fn hold_stake(&self, stake: Currency) {
        self.balance.fetch_sub(stake.0 as isize, Relaxed);
    }

    pub fn release_stake(&self, stake: Currency) {
        self.balance.fetch_add(stake.0 as isize, Relaxed);
    }
}
