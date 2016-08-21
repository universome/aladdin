use base::Currency;
use events::Event;

pub trait Gambler {
    fn authorize() -> Self;
    fn check_balance(&self) -> Currency;
    fn get_events(&self) -> Vec<Event>;
    fn make_bet(&self, event: Event, outcome: u32, size: Currency);

    fn reset_cache(&self) {}
}
