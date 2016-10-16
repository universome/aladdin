use base::error::Result;
use base::currency::Currency;
use markets::{Offer, Outcome};

mod egamingbets;
mod vitalbet;
mod xsporta;
mod cybbet;
mod betway;
mod betclub;

pub trait Gambler {
    fn authorize(&self, username: &str, password: &str) -> Result<()>;
    fn check_balance(&self) -> Result<Currency>;
    fn watch(&self, cb: &Fn(Offer, bool)) -> Result<()>;
    fn place_bet(&self, offer: Offer, outcome: Outcome, stake: Currency) -> Result<()>;
    fn check_offer(&self, offer: &Offer, outcome: &Outcome, stake: Currency) -> Result<bool> {
        Ok(true)
    }
}

pub type BoxedGambler = Box<Gambler + Send + Sync>;

macro_rules! gambler_map {
    ($host:expr, $( $pat:pat => $module:ident::$gambler:ident ),*) => {
        match $host {
            $($pat => (
                concat!(module_path!(), "::", stringify!($module)),
                Box::new($module::$gambler::new()) as BoxedGambler
            ),)*
            _ => panic!("There is no gambler for {}", $host)
        }
    }
}

pub fn new(host: &str) -> (&'static str, BoxedGambler) {
    gambler_map!(host,
        "egamingbets" => egamingbets::EGB,
        "vitalbet" => vitalbet::VitalBet,
        "1xsporta" => xsporta::XBet,
        "cybbet" => cybbet::CybBet,
        "betway" => betway::BetWay,
        "betclub" => betclub::BetClub
    )
}
