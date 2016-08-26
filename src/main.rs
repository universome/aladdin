extern crate chrono;
#[macro_use]
extern crate hyper;
extern crate kuchiki;
extern crate regex;
#[macro_use]
extern crate lazy_static;
extern crate url;
#[macro_use]
extern crate mime;
extern crate rustc_serialize;
extern crate toml;
extern crate crossbeam;

mod base;
mod events;
mod gamblers;
mod opportunity;
mod arbitrer;

fn main() {
    arbitrer::run();
}
