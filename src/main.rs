#[macro_use]
extern crate log;
extern crate env_logger;
extern crate time;
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
    env_logger::init().unwrap();

    // TODO(loyd): make CLI.
    arbitrer::run();
}
