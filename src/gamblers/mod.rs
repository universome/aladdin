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

macro_rules! gambler_map {
    ($host:expr, $( $pat:pat => $gambler:expr ),*) => {
        match $host {
            $($pat => Box::new($gambler) as Box<Gambler + Sync>,)*
            _ => panic!("There is no gambler for {}", $host)
        }
    }
}

pub fn new(host: &str) -> Box<Gambler + Sync> {
    gambler_map!(host,
        "egamingbets.com" => egamingbets::EGB::new(),
        "vitalbet.com" => vitalbet::VitalBet::new(),
        "1xsporta.space" => xsporta::XBet::new(),
        "cybbet.com" => cybbet::CybBet::new()
    )
}
