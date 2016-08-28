use std::collections::HashMap;
use crossbeam;
use crossbeam::sync::MsQueue;
use time;

use base::config::CONFIG;
use events::Offer;
use gamblers::{self, Gambler};
use opportunity::{self, Strategy};

struct Bookie {
    host: String,
    username: String,
    password: String,
    gambler: Box<Gambler + Sync>
}

struct MarkedOffer<'a>(&'a Bookie, Offer);
type Event<'a> = Vec<MarkedOffer<'a>>;

pub fn run() {
    let bookies = init_bookies();
    let incoming = &MsQueue::new();
    let outgoing = &MsQueue::new();

    crossbeam::scope(|scope| {
        for bookie in &bookies {
            scope.spawn(move || run_gambler(&bookie, &incoming, &outgoing));
        }

        process_queues(&incoming, &outgoing);
    });
}

fn init_bookies() -> Vec<Bookie> {
    let mut bookies = vec![];
    let array = CONFIG.lookup("bookies").unwrap().as_slice().unwrap();

    for item in array {
        let enabled = item.lookup("enabled").map_or(true, |x| x.as_bool().unwrap());

        if !enabled {
            continue;
        }

        let host = item.lookup("host").unwrap().as_str().unwrap();
        let username = item.lookup("username").unwrap().as_str().unwrap();
        let password = item.lookup("password").unwrap().as_str().unwrap();

        bookies.push(Bookie {
            host: host.to_owned(),
            username: username.to_owned(),
            password: password.to_owned(),
            gambler: gamblers::new(host)
        });
    }

    bookies
}

fn run_gambler<'a>(bookie: &'a Bookie,
                   incoming: &MsQueue<MarkedOffer<'a>>,
                   outgoing: &MsQueue<MarkedOffer<'a>>)
{
    // TODO(loyd): add error handling (don't forget about catching panics!).
    if let Err(error) = bookie.gambler.authorize(&bookie.username, &bookie.password) {
        println!("Auth {}: {}", bookie.host, error);
        return;
    }

    let error = bookie.gambler.watch(&|offer, update| {
        let marked = MarkedOffer(bookie, offer);
        if update { incoming.push(marked); } else { outgoing.push(marked); }
    }).unwrap_err();

    println!("{}: {}", bookie.host, error);
}

fn process_queues(incoming: &MsQueue<MarkedOffer>, outgoing: &MsQueue<MarkedOffer>) {
    let mut events = HashMap::new();

    loop {
        let marked = incoming.pop();
        let key = marked.1.clone();
        println!("Updated [{}]: {:?}", marked.0.host, marked.1.kind);
        update_offer(&mut events, marked);

        while let Some(marked) = outgoing.try_pop() {
            println!("Updated [{}]: {:?}", marked.0.host, marked.1.kind);
            remove_offer(&mut events, marked);
        }

        if let Some(event) = events.get(&key) {
            realize_event(event);
        }
    }
}

fn remove_offer(events: &mut HashMap<Offer, Event>, marked: MarkedOffer) {
    let mut remove_event = false;

    if let Some(event) = events.get_mut(&marked.1) {
        let index = event.iter()
            .position(|stored| stored.0 as *const _ == marked.0 as *const _);

        if let Some(index) = index {
            event.swap_remove(index);
        }

        remove_event = !event.is_empty();
    }

    if remove_event {
        events.remove(&marked.1);
    }
}

fn update_offer<'i>(events: &mut HashMap<Offer, Event<'i>>, marked: MarkedOffer<'i>) {
    if events.contains_key(&marked.1) {
        let event = events.get_mut(&marked.1).unwrap();

        let index = event.iter()
            .position(|stored| stored.0 as *const _ == marked.0 as *const _);

        if let Some(index) = index {
            event[index] = marked;
        } else {
            event.push(marked);
        }
    } else {
        events.insert(marked.1.clone(), vec![marked]);
    }
}

fn realize_event(event: &Event) {
    println!("{}, {:?}:", format_date(event[0].1.date, "%d/%m"), event[0].1.kind);

    for &MarkedOffer(vendor, ref offer) in event {
        print!("    {:20} {}", vendor.host, format_date(offer.date, "%R"));

        for outcome in &offer.outcomes {
            print!(" {:15}", outcome.0);
        }

        for outcome in &offer.outcomes {
            print!(" {:.2}", outcome.1);
        }

        print!("\n");
    }

    let outcomes = event.into_iter().map(|o| o.1.outcomes.as_slice());
    let opp = opportunity::find_best(outcomes, Strategy::Unbiased);

    if let Some(opp) = opp {
        println!("  => There is an opportunity: {:?}", opp);
    }

    print!("\n");
}

fn format_date(date: u32, format: &str) -> String {
    time::strftime(format, &time::at_utc(time::Timespec::new(date as i64, 0)).to_local()).unwrap()
}
