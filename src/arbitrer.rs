use std::collections::HashMap;
use std::cmp;
use std::thread;
use chrono::{UTC, Duration};
use crossbeam;
use crossbeam::sync::MsQueue;

use base::CONFIG;
use events::Offer;
use gamblers::{self, Gambler};
use opportunity::{self, Strategy};

struct GamblerInfo {
    host: String,
    gambler: Box<Gambler + Sync>
}

struct MarkedOffer(usize, Offer);
type Event = Vec<MarkedOffer>;

pub fn run() {
    let gamblers = init_gamblers();
    let (min_delay, max_delay) = get_delay_window();

    loop {
        let events = fetch_events(&gamblers);
        realize_events(&gamblers, &events);

        let delay = clamp(min_delay, find_delay(&events), max_delay);
        println!("Sleep for {}m", delay.num_minutes());
        thread::sleep(delay.to_std().unwrap());
    }
}

fn init_gamblers() -> Vec<GamblerInfo> {
    let mut info = vec![];
    let array = CONFIG.lookup("gamblers").unwrap().as_slice().unwrap();

    for item in array {
        let enabled = item.lookup("enabled").map_or(true, |x| x.as_bool().unwrap());

        if !enabled {
            continue;
        }

        let host = item.lookup("host").unwrap().as_str().unwrap();
        let username = item.lookup("username").unwrap().as_str().unwrap();
        let password = item.lookup("password").unwrap().as_str().unwrap();

        let gambler = gamblers::new(host);

        // TODO(loyd): parallel authorization.
        if let Err(error) = gambler.authorize(username, password) {
            println!("Error during auth {}: {}", host, error);
            continue;
        }

        println!("Authorized: {}", host);

        info.push(GamblerInfo {
            host: host.to_owned(),
            gambler: gambler
        });
    }

    info
}

fn get_delay_window() -> (Duration, Duration) {
    let min = CONFIG.lookup("arbitrer.min-delay").unwrap().as_integer().unwrap();
    let max = CONFIG.lookup("arbitrer.max-delay").unwrap().as_integer().unwrap();

    (Duration::minutes(min), Duration::minutes(max))
}

fn fetch_events(gamblers: &[GamblerInfo]) -> Vec<Event> {
    let queue = &MsQueue::new();

    let events = crossbeam::scope(|scope| {
        for (idx, info) in gamblers.iter().enumerate() {
            scope.spawn(move || {
                let result = info.gambler.fetch_offers();

                if let Err(ref error) = result {
                    println!("Error during fetching from {}: {}", info.host, error);
                    queue.push(None);
                    return;
                }

                let result = result.unwrap();
                if result.is_empty() {
                    println!("There is no offers from {}", info.host);
                    queue.push(None);
                    return;
                }

                for offer in result {
                    queue.push(Some(MarkedOffer(idx, offer)))
                }

                queue.push(None);
            });
        }

        group_offers(queue, gamblers.len())
    });

    events.into_iter().map(|(_, e)| e).collect()
}

fn group_offers(queue: &MsQueue<Option<MarkedOffer>>, mut count: usize) -> HashMap<Offer, Event> {
    let mut events: HashMap<_, Event> = HashMap::new();

    while count > 0 {
        let marked = queue.pop();

        if marked.is_none() {
            count -= 1;
            continue;
        }

        let marked = marked.unwrap();

        if events.contains_key(&marked.1) {
            events.get_mut(&marked.1).unwrap().push(marked);
        } else {
            // TODO(loyd): how to avoid copying?
            events.insert(marked.1.clone(), vec![marked]);
        }
    }

    events
}

fn realize_events(gamblers: &[GamblerInfo], events: &[Event]) {
    for (i, event) in events.iter().enumerate() {
        println!("[#{}] {}, {:?}:", i, event[0].1.date, event[0].1.kind);

        for offer in event {
            println!("  {}: {:?}", gamblers[offer.0].host, offer.1.outcomes);
        }

        let outcomes = event.into_iter().map(|o| o.1.outcomes.as_slice());
        let opp = opportunity::find_best(outcomes, Strategy::Unbiased);

        if let Some(opp) = opp {
            println!("  There is opportunity: {:?}", opp);
        }
    }
}

fn find_delay(events: &[Event]) -> Duration {
    let nearest = events.iter().map(|b| b[0].1.date).min().unwrap();
    let now = UTC::now();

    nearest - now
}

fn clamp<T: cmp::Ord>(min: T, val: T, max: T) -> T {
    cmp::max(min, cmp::min(val, max))
}
