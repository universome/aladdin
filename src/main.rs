#![feature(plugin, custom_derive)]
#![plugin(serde_macros)]

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
#[macro_use]
extern crate url;
#[macro_use]
extern crate mime;
extern crate serde;
extern crate serde_json;
extern crate toml;
extern crate crossbeam;

mod base;
mod events;
mod gamblers;
mod opportunity;
mod arbitrer;

fn main() {
    base::logger::init().unwrap();

    // TODO(loyd): make CLI.
    arbitrer::run();
}
