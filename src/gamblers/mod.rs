use base::Prime;
use base::Currency;
use events::{Event, Outcome};

pub use self::egamingbets::EGB;
pub use self::vitalbet::VitalBet;

mod egamingbets;
mod vitalbet;

pub trait Gambler {
    fn authorize(&self, username: &str, password: &str) -> Prime<()>;
    fn check_balance(&self) -> Prime<Currency>;
    fn get_events(&self) -> Prime<Vec<Event>>;
    fn make_bet(&self, event: Event, outcome: Outcome, bet: Currency) -> Prime<()>;

    fn reset_cache(&self) {}
}
