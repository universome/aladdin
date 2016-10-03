#![allow(non_snake_case)]

use serde_json as json;
use std::collections::{HashMap, HashSet};

use base::error::{Result};
use base::session::Session;
use base::timers::Periodic;
use base::currency::Currency;
use gamblers::Gambler;
use events::{Offer, Outcome, Kind, DRAW};
use events::kinds::*;

pub struct BetClub {
    session: Session
}

impl BetClub {
    pub fn new() -> BetClub {
        BetClub {
            session: Session::new("betclub2.com")
        }
    }
}

impl Gambler for BetClub {
    fn authorize(&self, username: &str, password: &str) -> Result<()> {
        let path = "/WebServices/BRService.asmx/LogIn";
        let request_data = AuthRequest {
            login: username,
            password: password
        };

        self.session.post_json(path, request_data).map(|_| ())
    }

    fn check_balance(&self) -> Result<Currency> {
        let path = "/WebServices/BRService.asmx/GetUserBalance";
        let response = try!(self.session.post_as_json(path, ""));
        let balance: BalanceResponse = try!(json::from_reader(response));

        Ok(Currency::from(balance.d.Amount))
    }

    fn watch(&self, cb: &Fn(Offer, bool)) -> Result<()> {
        let mut offers: HashMap<u64, Offer> = HashMap::new();
        let path = "/WebServices/BRService.asmx/GetTournamentEventsBySportByDuration";
        let body = EventsRequest {culture: "en-us", sportId: 300, countHours: "12"};

        // TODO(universome): Add fluctuations (cloudflare can spot us)
        for _ in Periodic::new(30) {
            let response = try!(self.session.post_json(path, &body));
            let tournaments = try!(json::from_reader::<_, TournamentsResponse>(response)).d;
            let fresh_events: Vec<_> = tournaments.into_iter()
                .flat_map(|t| t.EventsHeaders)
                .collect();
            let fresh_offers: Vec<_> = fresh_events.iter().filter_map(get_offer).collect();
            let fresh_ids: HashSet<_> = fresh_offers.iter().map(|o| o.inner_id).collect();

            // Remove outdated offers
            let outdated_ids: HashSet<_> = offers.keys()
                .filter(|id| !fresh_ids.contains(id))
                .map(|id| id.clone())
                .collect();

            for id in outdated_ids {
                let offer = offers.remove(&id).unwrap();
                cb(offer, false);
            }

            // Gather new offers
            for fresh_offer in fresh_offers {
                if let Some(offer) = offers.get(&fresh_offer.inner_id) {
                    if offer == &fresh_offer && offer.date == fresh_offer.date {
                        continue; // It's just the same offer :(
                    }
                }

                cb(fresh_offer.clone(), true);
                offers.insert(fresh_offer.inner_id, fresh_offer);
            }
        }

        Ok(())
    }

    fn place_bet(&self, offer: Offer, outcome: Outcome, stake: Currency) -> Result<()> {
        unimplemented!();
    }
}

#[derive(Serialize, Debug)]
struct AuthRequest<'a> {
    login: &'a str ,
    password: &'a str
}

#[derive(Deserialize, Debug)]
struct BalanceResponse {
    d: Balance
}

#[derive(Deserialize, Debug)]
struct Balance {
    Amount: f64
}

#[derive(Deserialize, Debug)]
struct TournamentsResponse {
    d: Vec<Tournament>
}

#[derive(Deserialize, Debug)]
struct Tournament {
    EventsHeaders: Vec<Event>,
}

#[derive(Deserialize, Debug)]
struct Event {
    Id: u32,
    Date: String,
    TeamsGroup: Vec<String>,
    CountryName: String,
    Markets: Vec<Market>
}

#[derive(Deserialize, Debug)]
struct Market {
    Id: String,
    IsEnabled: bool,
    IsMain: bool,
    Rates: Vec<Rate>,
    Caption: String
}

#[derive(Deserialize, Debug)]
struct Rate {
    NameShort: String,
    AddToBasket: RateBasket
}

#[derive(Deserialize, Debug)]
struct RateBasket {
    eId: u32,
    bId: u32,
    r: f64,
    isLive: bool
}

#[derive(Serialize, Debug)]
struct EventsRequest<'a> {
    culture: &'a str,
    countHours: &'a str,
    sportId: u32
}

fn get_offer(event: &Event) -> Option<Offer> {
    let market = match event.Markets.iter().find(|m| m.IsMain) {
        Some(m) => m,
        None => {
            warn!("There is no main market in event: {:?}", event);
            return None;
        }
    };

    if !market.IsEnabled || market.Caption != "Result" {
        return None;
    }

    let outcomes = match get_outcomes(event, &market) {
        Some(outcomes) => outcomes,
        None => return None
    };

    let kind = match event.CountryName.as_ref() {
        "Dota II" => Kind::Dota2(Dota2::Series),
        "StarCraft II" => Kind::StarCraft2(StarCraft2::Series),
        "Counter-Strike" => Kind::CounterStrike(CounterStrike::Series),
        "Heroes Of The Storm" => Kind::HeroesOfTheStorm(HeroesOfTheStorm::Series),
        "League of Legends" => Kind::LeagueOfLegends(LeagueOfLegends::Series),
        "Overwatch" => Kind::Overwatch(Overwatch::Series),
        "World of Tanks" => Kind::WorldOfTanks(WorldOfTanks::Series),
        unsupported_type => {
            warn!("Found new type: {}", unsupported_type);
            return None;
        }
    };

    let date: u32 = match event.Date.trim_left_matches("/Date(").trim_right_matches(")/")
        .parse::<u64>() {
            Ok(ts) => (ts / 1000) as u32,
            Err(err) => {
                warn!("Failed to parse date format: {}", event.Date);
                return None;
            }
        };

    Some(Offer {
        inner_id: event.Id as u64,
        outcomes: outcomes,
        kind: kind,
        date: date
    })
}

fn get_outcomes(event: &Event, market: &Market) -> Option<Vec<Outcome>> {
    if !market.Rates.len() < 2 || event.TeamsGroup.len() < 2 {
        return None;
    }

    let mut outcomes = vec![
        Outcome(event.TeamsGroup[0].clone(), market.Rates[0].AddToBasket.r),
        Outcome(event.TeamsGroup[1].clone(), market.Rates.last().unwrap().AddToBasket.r)
    ];

    if market.Rates.len() == 3 {
        outcomes.push(Outcome(DRAW.to_owned(), market.Rates[1].AddToBasket.r));
    }

    Some(outcomes)
}
