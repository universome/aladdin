#![feature(plugin, custom_derive, static_in_const, specialization, conservative_impl_trait)]
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
extern crate websocket;
extern crate rusqlite;
extern crate backtrace;
extern crate parking_lot;

use std::thread;

mod constants;
mod base;
mod markets;
mod gamblers;
mod arbitrer;
mod server;
mod combo;

fn main() {
    base::logger::init().unwrap();

    thread::Builder::new()
        .name("server".to_owned())
        .spawn(server::run)
        .unwrap();

    arbitrer::run();
}
