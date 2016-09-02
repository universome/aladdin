#![allow(non_snake_case)]

use std::collections::{HashMap, HashSet};

use base::error::Result;
use base::timers::Periodic;
use base::session::Session;
use base::currency::Currency;
use gamblers::Gambler;
use events::{Offer, Outcome, DRAW, Kind};
use events::{CounterStrike, Dota2, LeagueOfLegends, Overwatch, StarCraft2, WorldOfTanks};

pub struct XBet {
    session: Session
}

impl XBet {
    pub fn new() -> XBet {
        XBet {
            session: Session::new("https://1xsporta.space")
        }
    }
}

impl Gambler for XBet {
    fn authorize(&self, username: &str, password: &str) -> Result<()> {
        unimplemented!();
    }

    fn check_balance(&self) -> Result<Currency> {
        unimplemented!();
    }

    fn watch(&self, cb: &Fn(Offer, bool)) -> Result<()> {
        let path = "/LineFeed/Get1x2?sportId=40&count=50&cnt=10&lng=en";
        let mut map = HashMap::new();

        for _ in Periodic::new(60) {
            let message = try!(self.session.get_json::<Message>(path));
            let offers = try!(grab_offers(message));

            let active = offers.iter()
                .map(|o| o.inner_id)
                .collect::<HashSet<_>>();

            // Remove redundant offers.
            let redundants = map.keys()
                .filter(|id| !active.contains(id))
                .map(|id| *id)
                .collect::<Vec<_>>();

            for id in redundants {
                let offer = map.remove(&id).unwrap();
                cb(offer, false);
            }

            // Add/update offers.
            for offer in offers {
                if !map.contains_key(&offer.inner_id) {
                    map.insert(offer.inner_id, offer.clone());
                }

                debug_assert!(offer == map[&offer.inner_id]);
                cb(offer, true);
            }
        }

        Ok(())
    }

    fn place_bet(&self, offer: Offer, outcome: Outcome, bet: Currency) -> Result<()> {
        unimplemented!();
    }
}

#[derive(Deserialize)]
struct Message {
    Error: String,
    Success: bool,
    Value: Vec<Info>
}

#[derive(Deserialize)]
struct Info {
    // TODO(loyd): what is the difference between `ConstId`, `Id` and `MainGameId`?
    Id: u32,
    ChampEng: String,
    Opp1: String,
    Opp2: String,
    Start: u32,
    Events: Vec<Event>
}

#[derive(Deserialize)]
struct Event {
    C: f64,
    T: u32
}

fn grab_offers(message: Message) -> Result<Vec<Offer>> {
    if !message.Success {
        return Err(From::from(message.Error));
    }

    let offers = message.Value.into_iter().filter_map(|info| {
        let coef_1 = info.Events.iter().find(|ev| ev.T == 1).map(|ev| ev.C);
        let coef_2 = info.Events.iter().find(|ev| ev.T == 3).map(|ev| ev.C);

        if coef_1.is_none() || coef_2.is_none() {
            return None;
        }

        let kind = match &info.ChampEng[..4] {
            "CS:G" | "Coun" => Kind::CounterStrike(CounterStrike::Series),
            "Dota" => Kind::Dota2(Dota2::Series),
            "Leag" => Kind::LeagueOfLegends(LeagueOfLegends::Series),
            "Star" => Kind::StarCraft2(StarCraft2::Series),
            "Worl" => Kind::WorldOfTanks(WorldOfTanks::Series),
            _ => {
                debug!("Unknown kind: {}", info.ChampEng);
                return None;
            }
        };

        let coef_draw = info.Events.iter().find(|ev| ev.T == 2).map(|ev| ev.C);
        let date = info.Start;
        let id = info.Id;

        let mut outcomes = vec![
            Outcome(info.Opp1, coef_1.unwrap()),
            Outcome(info.Opp2, coef_2.unwrap())
        ];

        if let Some(coef) = coef_draw {
            outcomes.push(Outcome(DRAW.to_owned(), coef));
        }

        Some(Offer {
            date: date,
            kind: kind,
            outcomes: outcomes,
            inner_id: id as u64
        })
    }).collect();

    Ok(offers)
}