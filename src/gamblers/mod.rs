use base::error::Result;
use base::currency::Currency;
use events::{Offer, Outcome};

mod egamingbets;
mod vitalbet;
mod xsporta;
mod cybbet;

pub trait Gambler {
    fn authorize(&self, username: &str, password: &str) -> Result<()>;
    fn check_balance(&self) -> Result<Currency>;
    fn watch(&self, cb: &Fn(Offer, bool)) -> Result<()>;
    fn place_bet(&self, offer: Offer, outcome: Outcome, bet: Currency) -> Result<()>;
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

pub fn new(host: &str) -> (&str, BoxedGambler) {
    gambler_map!(host,
        "egamingbets.com" => egamingbets::EGB,
        "vitalbet.com" => vitalbet::VitalBet,
        "1xsporta.space" => xsporta::XBet,
        "cybbet.com" => cybbet::CybBet
    )
}
