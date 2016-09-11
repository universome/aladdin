use std::collections::HashMap;
use std::sync::mpsc::{self, Sender, Receiver};
use crossbeam;

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

    let (incoming_tx, incoming_rx) = mpsc::channel();
    let (outgoing_tx, outgoing_rx) = mpsc::channel();

    crossbeam::scope(|scope| {
        for bookie in &bookies {
            let incoming_tx = incoming_tx.clone();
            let outgoing_tx = outgoing_tx.clone();

            scope.spawn(move || run_gambler(bookie, incoming_tx, outgoing_tx));
        }

        process_channels(incoming_rx, outgoing_rx);
    });
}

fn init_bookies() -> Vec<Bookie> {
    let mut bookies = Vec::new();
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
                   incoming: Sender<MarkedOffer<'a>>,
                   outgoing: Sender<MarkedOffer<'a>>)
{
    // TODO(loyd): add error handling (don't forget about catching panics!).
    if let Err(error) = bookie.gambler.authorize(&bookie.username, &bookie.password) {
        error!("Auth {}: {}", bookie.host, error);
        return;
    }

    let error = bookie.gambler.watch(&|offer, update| {
        let marked = MarkedOffer(bookie, offer);

        if update {
            incoming.send(marked).unwrap();
        } else {
            outgoing.send(marked).unwrap();
        }
    }).unwrap_err();

    error!("{}: {}", bookie.host, error);
}

fn process_channels(incoming: Receiver<MarkedOffer>, outgoing: Receiver<MarkedOffer>) {
    let mut events = HashMap::new();

    loop {
        let marked = incoming.recv().unwrap();
        let key = marked.1.clone();
        update_offer(&mut events, marked);

        while let Ok(marked) = outgoing.try_recv() {
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
            debug!("{} by {} is removed", marked.1, marked.0.host);
        } else {
            warn!("There is no {} by {}", marked.1, marked.0.host);
        }

        remove_event = event.is_empty();
    }

    if remove_event {
        debug!("Event [{} by {}] is removed", marked.1, marked.0.host);
        events.remove(&marked.1);
    }
}

fn update_offer<'i>(events: &mut HashMap<Offer, Event<'i>>, marked: MarkedOffer<'i>) {
    if events.contains_key(&marked.1) {
        let event = events.get_mut(&marked.1).unwrap();

        let index = event.iter()
            .position(|stored| stored.0 as *const _ == marked.0 as *const _);

        if let Some(index) = index {
            if marked.1.outcomes.len() != event[index].1.outcomes.len() {
                error!("{} by {} is NOT updated: incorrect dimension", marked.1, marked.0.host);
                return;
            }

            debug!("{} by {} is updated", marked.1, marked.0.host);
            event[index] = marked;
        } else {
            if marked.1.outcomes.len() != event[0].1.outcomes.len() {
                error!("{} by {} is NOT added: incorrect dimension", marked.1, marked.0.host);
                return;
            }

            debug!("{} by {} is added", marked.1, marked.0.host);
            event.push(marked);
        }
    } else {
        debug!("Event [{} by {}] is added", marked.1, marked.0.host);
        events.insert(marked.1.clone(), vec![marked]);
    }
}

fn realize_event(event: &Event) {
    if event.len() < 2 {
        return;
    }

    info!("Checking event:");

    for &MarkedOffer(bookie, ref offer) in event {
        info!("    {} by {}", offer, bookie.host);
    }

    let outcomes = event.into_iter().map(|o| o.1.outcomes.as_slice());
    let opp = opportunity::find_best(outcomes, Strategy::Unbiased);

    if let Some(opp) = opp {
        info!("  => There is an opportunity: {:?}", opp);
    }
}
