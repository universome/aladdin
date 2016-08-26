use std::result;
use std::error;

pub use self::config::CONFIG;
pub use self::session::Session;
pub use self::currency::Currency;
pub use self::parsing::{NodeRefExt, ElementDataExt};

mod config;
mod session;
mod currency;
mod parsing;

pub type Prime<T> = result::Result<T, Box<error::Error>>;
