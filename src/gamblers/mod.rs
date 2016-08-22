use base::Prime;
use base::Currency;
use events::Event;

pub trait Gambler {
    fn authorize(&self, username: &str, password: &str) -> Prime<()>;
    fn check_balance(&self) -> Prime<Currency>;
    fn get_events(&self) -> Prime<Vec<Event>>;
    fn make_bet(&self, event: Event, outcome: u32, size: Currency) -> Prime<()>;

    fn reset_cache(&self) {}
}
