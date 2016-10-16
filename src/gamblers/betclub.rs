#![allow(non_snake_case)]

use std::io::Read;
use std::sync::Mutex;
use std::collections::{HashMap, HashSet};
use serde_json as json;

use base::error::{Result};
use base::session::Session;
use base::timers::Periodic;
use base::currency::Currency;
use gamblers::Gambler;
use events::{OID, Offer, Outcome, Game, Kind, DRAW};

pub struct BetClub {
    session: Session,
    state: Mutex<State>
}

struct State {
    offers: HashMap<u64, Offer>,
    events: Vec<Event>
}

impl BetClub {
    pub fn new() -> BetClub {
        BetClub {
            session: Session::new("betclub3.com"),
            state: Mutex::new(State {
                offers: HashMap::new(),
                events: Vec::new()
            })
        }
    }

    fn fetch_events(&self) -> Result<Vec<Event>> {
        let path = "/WebServices/BRService.asmx/GetTournamentEventsBySportByDuration";
        let body = EventsRequest {culture: "en-us", sportId: 300, countHours: "12"};

        let response = try!(self.session.post_json(path, &body));
        let tournaments = try!(json::from_reader::<_, TournamentsResponse>(response)).d;

        Ok(tournaments.into_iter().flat_map(|t| t.EventsHeaders).collect())
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
        // TODO(universome): Add fluctuations (cloudflare can spot us)
        for _ in Periodic::new(30) {
            let mut state = self.state.lock().unwrap();

            state.events = try!(self.fetch_events());

            let fresh_offers: Vec<_> = state.events.iter().filter_map(get_offer).collect();
            let fresh_ids: HashSet<_> = fresh_offers.iter().map(|o| o.oid).collect();

            // Remove outdated offers
            let outdated_ids: HashSet<_> = state.offers.keys()
                .filter(|id| !fresh_ids.contains(id))
                .map(|id| id.clone())
                .collect();

            for id in outdated_ids {
                let offer = state.offers.remove(&id).unwrap();
                cb(offer, false);
            }

            // Gather new offers
            for fresh_offer in fresh_offers {
                if let Some(offer) = state.offers.get(&fresh_offer.oid) {
                    if offer == &fresh_offer && offer.date == fresh_offer.date {
                        continue; // It's just the same offer :(
                    }
                }

                cb(fresh_offer.clone(), true);
                state.offers.insert(fresh_offer.oid, fresh_offer);
            }
        }

        Ok(())
    }

    fn check_offer(&self, offer: &Offer, outcome: &Outcome, stake: Currency) -> Result<bool> {
        let current_events = try!(self.fetch_events());
        let event = current_events.iter().find(|e| e.Id == offer.oid as u32).unwrap();

        match get_offer(event) {
            Some(offer) => Ok(offer.outcomes.into_iter().find(|o| o == outcome).is_some()),
            None => Ok(false)
        }
    }

    fn place_bet(&self, offer: Offer, outcome: Outcome, stake: Currency) -> Result<()> {
        let stake: f64 = stake.into();
        let state = try!(self.state.lock());
        let event = state.events.iter().find(|e| e.Id == offer.oid as u32).unwrap();
        let market = event.get_market().unwrap();

        let basket = if outcome.0 == event.TeamsGroup[0] { &market.Rates[0].AddToBasket }
                     else if outcome.0 == event.TeamsGroup[1] { &market.Rates[2].AddToBasket }
                     else { &market.Rates[1].AddToBasket };

        // Add bet to betslip
        let body = format!(r#"{{
            "eId": {event_id},
            "bId": {bet_id},
            "r": {coef},
            "fs": {hand_size},
            "a1": {add_1},
            "a2": {add_2},
            "isLive": {is_live},
            "culture":"en-us"
        }}"#,
            event_id = basket.eId,
            bet_id = basket.bId,
            hand_size = match basket.fs { Some(v) => v.to_string(), _ => "null".to_string() },
            add_1 = match basket.a1 { Some(v) => v.to_string(), _ => "null".to_string() },
            add_2 = match basket.a2 { Some(v) => v.to_string(), _ => "null".to_string() },
            coef = basket.r,
            is_live = basket.isLive
        );

        let path = "/WebServices/BRService.asmx/AddToBetslip";
        let mut response = try!(self.session.post_as_json(path, body.as_ref()));
        let mut string = String::new();

        try!(response.read_to_string(&mut string));

        if !string.contains("LinesID") {
            return Err(From::from(string));
        }

        // Place bet
        let body = format!(r#"{{
            "betAmount": {stake},
            "systemIndex": -1,
            "statuses": {{"{event_id}_{bet_id}_{hand_size}_{add_1}_{add_2}": true}},
            "doAcceptOddsChanges": false
        }}"#,
            stake = stake,
            event_id = basket.eId,
            bet_id = basket.bId,
            hand_size = match basket.fs { Some(v) => v.to_string(), _ => "null".to_string() },
            add_1 = match basket.a1 { Some(v) => v.to_string(), _ => "null".to_string() },
            add_2 = match basket.a2 { Some(v) => v.to_string(), _ => "null".to_string() }
        );

        let path = "/WebServices/BRService.asmx/PlaceBet";
        let mut response = try!(self.session.post_as_json(path, body.as_ref()));
        let mut string = String::new();

        try!(response.read_to_string(&mut string));

        if !string.contains("AmountIn") {
            return Err(From::from(string));
        }

        Ok(())
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

impl Event {
    fn get_market(&self) -> Option<&Market> {
        let market = match self.Markets.iter().find(|m| m.IsMain) {
            Some(m) => m,
            None => return None
        };

        if !market.IsEnabled
        || !market.Rates.len() < 2
        || market.Caption != "Result"
        || self.TeamsGroup.len() < 2 {
            return None;
        }

        Some(market)
    }
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
    AddToBasket: Basket
}

#[derive(Deserialize, Debug)]
struct Basket {
    eId: u32,
    bId: u32,
    r: f64,
    isLive: bool,
    a1: Option<u32>,
    a2: Option<u32>,
    fs: Option<f64>
}

#[derive(Serialize, Debug)]
struct EventsRequest<'a> {
    culture: &'a str,
    countHours: &'a str,
    sportId: u32
}

fn get_offer(event: &Event) -> Option<Offer> {
    let market = match event.get_market() {
        Some(m) => m,
        None => return None
    };

    let outcomes = match get_outcomes(event, &market) {
        Some(outcomes) => outcomes,
        None => return None
    };

    let game = match event.CountryName.as_ref() {
        "Dota II" => Game::Dota2,
        "StarCraft II" => Game::StarCraft2,
        "Counter-Strike" => Game::CounterStrike,
        "Heroes Of The Storm" => Game::HeroesOfTheStorm,
        "Hearthstone" => Game::Hearthstone,
        "League of Legends" => Game::LeagueOfLegends,
        "Overwatch" => Game::Overwatch,
        "World of Tanks" => Game::WorldOfTanks,
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
        oid: event.Id as OID,
        outcomes: outcomes,
        game: game,
        kind: Kind::Series,
        date: date
    })
}

fn get_outcomes(event: &Event, market: &Market) -> Option<Vec<Outcome>> {
    let mut outcomes = vec![
        Outcome(event.TeamsGroup[0].clone(), market.Rates[0].AddToBasket.r),
        Outcome(event.TeamsGroup[1].clone(), market.Rates.last().unwrap().AddToBasket.r)
    ];

    if market.Rates.len() == 3 {
        outcomes.push(Outcome(DRAW.to_owned(), market.Rates[1].AddToBasket.r));
    }

    Some(outcomes)
}
