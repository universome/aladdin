use std::time::Duration;

use base::currency::Currency;

// TODO(loyd): reconsider after `const fn` stabilization.
lazy_static! {
    pub static ref RETRY_DELAY: Duration = Duration::new(30 * 60, 0);
    pub static ref CHECK_TIMEOUT: Duration = Duration::new(1, 0);

    pub static ref BASE_STAKE: Currency = Currency::from(1.00);
    pub static ref MAX_STAKE: Currency = Currency::from(5.00);
}

pub const HISTORY_SIZE: u32 = 20;

pub const MIN_PROFIT: f64 = 0.02;
pub const MAX_PROFIT: f64 = 0.15;

pub const DATABASE: &str = "aladdin.db";

pub const PORT: u16 = 3042;
pub const COMBO_COUNT: u32 = 32;


pub const BOOKIES_AUTH: &[(&str, &str, &str)] = &[
    ("egamingbets.com", "shmaladdin",               "aladdin"),
    ("vitalbet.com",    "Алладин",                  "aladdin"),
    ("1xsporta.space",  "shmaladdin.rs@gmail.com",  "aladdin.rs"),
    ("cybbet.com",      "aladdin",                  "Shmaladdin42"),
    ("betway.com",      "shmaladdin",               "aladdin"),
    ("betclub2.com",    "shmaladdin.rs@gmail.com",  "aladdin571")
];
