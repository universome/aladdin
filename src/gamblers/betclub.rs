#![allow(non_snake_case)]

use std::sync::Mutex;
use std::collections::{HashMap, HashSet};

use base::error::{Result};
use base::session::Session;
use base::timers::Periodic;
use base::currency::Currency;
use gamblers::{Gambler, Message};
use gamblers::Message::*;
use markets::{OID, Offer, Outcome, Game, Kind, DRAW};

static SPORTS_IDS: &[u32] = &[1, 2, 3, 4, 5, 6, 8, 9, 12, 15, 16, 257, 279, 296, 300];

pub struct BetClub {
    session: Session,
    events: Mutex<HashMap<OID, Event>>
}

impl BetClub {
    pub fn new() -> BetClub {
        BetClub {
            session: Session::new("betclub3.com"),
            // TODO(universome): store only necessary info about the events.
            events: Mutex::new(HashMap::new())
        }
    }

    fn fetch_events(&self, sport_id: u32) -> Result<Vec<Event>> {
        let path = "/WebServices/BRService.asmx/GetTournamentEventsBySportByDuration";
        let body = EventsRequest { culture: "en-us", sportId: sport_id, countHours: "12" };

        let response: TournamentsResponse = try!(self.session.request(path).post(body));

        Ok(response.d.into_iter().flat_map(|t| t.EventsHeaders).collect())
    }
}

impl Gambler for BetClub {
    fn authorize(&self, username: &str, password: &str) -> Result<()> {
        let path = "/WebServices/BRService.asmx/LogIn";
        let request_data = AuthRequest {
            login: username,
            password: password
        };

        let response: String = try!(self.session.request(path).post(request_data));

        debug!("{}", response);

        Ok(())
    }

    fn check_balance(&self) -> Result<Currency> {
        let path = "/WebServices/BRService.asmx/GetUserBalance";
        let balance: BalanceResponse = try!(self.session.request(path).post("".to_string()));

        Ok(Currency::from(balance.d.Amount))
    }

    fn watch(&self, cb: &Fn(Message)) -> Result<()> {
        let mut active = SPORTS_IDS.iter().map(|_| HashSet::new()).collect::<Vec<_>>();

        for _ in Periodic::new(24) {
            for (sport_id, active) in SPORTS_IDS.iter().zip(active.iter_mut()) {
                let recent = try!(self.fetch_events(*sport_id));

                let data = recent.into_iter()
                    .filter_map(|event| get_offer(&event).map(|offer| (offer, event)))
                    .collect::<Vec<_>>();

                // Deactivate active offers.
                for &(ref offer, _) in &data {
                    active.remove(&offer.oid);
                }

                let mut events = self.events.lock().unwrap();

                // Now `active` contains inactive.
                for oid in active.drain() {
                    events.remove(&oid);
                    cb(Remove(oid));
                }

                // Add/update offers.
                for (offer, event) in data {
                    active.insert(offer.oid);
                    events.insert(offer.oid, event);
                    cb(Upsert(offer));
                }
            }
        }

        Ok(())
    }

    fn check_offer(&self, offer: &Offer, outcome: &Outcome, _: Currency) -> Result<bool> {
        let events = self.events.lock().unwrap();

        let sport_id = match events.get(&offer.oid) {
            Some(event) => event.SId,
            None => return Ok(false)
        };

        let current_events = try!(self.fetch_events(sport_id));
        let event = current_events.iter().find(|e| e.Id == offer.oid as u32).unwrap();

        match get_offer(event) {
            Some(offer) => Ok(offer.outcomes.into_iter().find(|o| o == outcome).is_some()),
            None => Ok(false)
        }
    }

    fn place_bet(&self, offer: Offer, outcome: Outcome, stake: Currency) -> Result<()> {
        let events = self.events.lock().unwrap();
        let event = &events[&offer.oid];
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
        let response: String = try!(self.session.request(path).post(body));

        if !response.contains("LinesID") {
            return Err(From::from(response));
        }

        let stake: f64 = stake.into();

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
        let response: String = try!(self.session.request(path).post(body));

        if !response.contains("AmountIn") {
            return Err(From::from(response));
        }

        Ok(())
    }
}

#[derive(Serialize)]
struct AuthRequest<'a> {
    login: &'a str ,
    password: &'a str
}

#[derive(Deserialize)]
struct BalanceResponse {
    d: Balance
}

#[derive(Deserialize)]
struct Balance {
    Amount: f64
}

#[derive(Deserialize)]
struct TournamentsResponse {
    d: Vec<Tournament>
}

#[derive(Deserialize)]
struct Tournament {
    EventsHeaders: Vec<Event>,
}

#[derive(Deserialize, Debug)]
struct Event {
    Id: u32,
    Date: String,
    TeamsGroup: Vec<String>,
    SId: u32,
    SportName: String,
    CountryName: String,
    Markets: Vec<Market>
}

impl Event {
    fn get_market(&self) -> Option<&Market> {
        let market = match self.Markets.iter().find(|m| m.IsMain) {
            Some(m) => m,
            None => return None
        };

        if !market.IsEnabled || market.Caption != "Result" || self.TeamsGroup.len() < 2 {
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

#[derive(Serialize)]
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

    let game = match get_game(event) {
        Some(game) => game,
        None => return None
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
    let x2 = if market.Rates.len() > 2 { 2 } else { 1 };

    let mut outcomes = vec![
        Outcome(event.TeamsGroup[0].clone(), market.Rates[0].AddToBasket.r),
        Outcome(event.TeamsGroup[1].clone(), market.Rates[x2].AddToBasket.r)
    ];

    if x2 == 2 {
        let draw_odds = market.Rates[1].AddToBasket.r;

        if draw_odds > 1. {
            outcomes.push(Outcome(DRAW.to_owned(), draw_odds));
        }
    }

    Some(outcomes)
}

fn get_game(event: &Event) -> Option<Game> {
    Some(match event.SportName.as_str() {
        "Basketball" => Game::Basketball,
        "Baseball" => Game::Baseball,
        "Tennis 3 set." | "Tennis 5-set." => Game::Tennis,
        "Soccer" => Game::Football,
        "Hockey" => Game::IceHockey,
        "Volleyball" => Game::Volleyball,
        "American football" => Game::AmericanFootball,
        "Handball" => Game::Handball,
        "Field hockey" => Game::FieldHockey,
        "Water polo" => Game::WaterPolo,
        "Badminton" => Game::Badminton,
        "Futsal" => Game::Futsal,
        "Snooker" => Game::Snooker,

        "Electronic Sports" => match event.CountryName.as_str() {
            "Dota II" => Game::Dota2,
            "StarCraft II" => Game::StarCraft2,
            "Counter-Strike" => Game::CounterStrike,
            "Heroes Of The Storm" => Game::HeroesOfTheStorm,
            "Hearthstone" => Game::Hearthstone,
            "League of Legends" => Game::LeagueOfLegends,
            "Overwatch" => Game::Overwatch,
            "World of Tanks" => Game::WorldOfTanks,
            unsupported => {
                warn!("Found new type: {}", unsupported);
                return None;
            }
        },

        name => {
            warn!("Unknown sport name: \"{}\"", name);
            return None;
        }
    })
}
