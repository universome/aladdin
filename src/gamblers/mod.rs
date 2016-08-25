use std::sync::Arc;

use base::Prime;
use base::Currency;
use events::{Offer, Outcome};

pub use self::egamingbets::EGB;
pub use self::vitalbet::VitalBet;

mod egamingbets;
mod vitalbet;

pub trait Gambler {
    fn authorize(&self, username: &str, password: &str) -> Prime<()>;
    fn check_balance(&self) -> Prime<Currency>;
    fn get_offers(&self) -> Prime<Vec<Offer>>;
    fn make_bet(&self, offer: Offer, outcome: Outcome, bet: Currency) -> Prime<()>;

    fn reset_cache(&self) {}
}

macro_rules! gambler_map {
    ($host:expr, $( $pat:pat => $gambler:expr ),*) => {
        match $host {
            $($pat => Arc::new($gambler) as Arc<Gambler>,)*
            _ => panic!("There is no gambler for {}", $host)
        }
    }
}

pub fn new(host: &str) -> Arc<Gambler> {
    gambler_map!(host,
        "egamingbets.com" => egamingbets::EGB::new(),
        "vitalbet.com" => vitalbet::VitalBet::new()
    )
}
