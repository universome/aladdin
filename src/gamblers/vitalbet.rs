#![allow(non_snake_case)]

use std::result::Result as StdResult;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use serde::{Deserialize, Deserializer};
use serde_json as json;
use time;
use url::percent_encoding::{utf8_percent_encode, USERINFO_ENCODE_SET};

use base::currency::Currency;
use base::timers::Periodic;
use base::error::{Result, Error};
use base::session::Session;
use gamblers::{Gambler, Message};
use gamblers::Message::*;
use markets::{OID, Offer, Outcome, Game, Kind, DRAW};

use self::PollingMessage as PM;

pub struct VitalBet {
    session: Session,
    state: Mutex<State>
}

define_encode_set! {
    pub VITALBET_ENCODE_SET = [USERINFO_ENCODE_SET] | {'+', '-'}
}

impl VitalBet {
    pub fn new() -> VitalBet {
        VitalBet {
            session: Session::new("ebettle.com"),
            state: Mutex::new(State {
                odds_to_events_ids: HashMap::new(),
                markets_to_events_ids: HashMap::new(),
                events: HashMap::new(),
                offers: HashMap::new(),
                changed_events: HashSet::new()
            })
        }
    }

    // TODO(universome): Pass timestamps, like they do.
    fn generate_polling_path(&self) -> Result<String> {
        // First, we should get connection token.
        let auth_path = concat!("/signalr/negotiate?transport=longPolling&clientProtocol=1.5",
                                "&connectionData=%5B%7B%22name%22%3A%22sporttypehub%22%7D%5D");
        let response: PollingAuthResponse = try!(self.session.request(auth_path).get());
        let token = response.ConnectionToken;
        let token = utf8_percent_encode(&token, VITALBET_ENCODE_SET).collect::<String>();

        // We should notify them, that we are starting polling (because they do it too).
        let start_polling_path = format!(concat!("/signalr/start?transport=longPolling",
                                 "&connectionData=%5B%7B%22name%22%3A%22sporttypehub%22%7D%5D",
                                 "clientProtocol=1.5&connectionToken={}"), token);
        try!(self.session.request(&start_polling_path).get::<String>());

        Ok(format!(concat!("/signalr/poll?transport=longPolling&clientProtocol=1.5",
                        "&connectionData=%5B%7B%22name%22%3A%22sporttypehub%22%7D%5D",
                        "&connectionToken={}"), token))
    }

    fn get_events(&self) -> Result<Vec<Event>> {
        let sports = try!(self.session.request("/api/sporttype/getallactive").get::<Vec<Sport>>());
        let events = sports.iter()
            .map(|s| s.ID)
            .filter_map(|sport_id| {
                let path = format!("/api/sportmatch/Get?sportID={}", sport_id);
                self.session.request(path.as_str()).get::<Vec<Event>>().ok()
            })
            .flat_map(|events| events)
            .collect::<Vec<_>>();

        trace!("Gathered {} events", events.len());

        Ok(events)
    }
}

impl Gambler for VitalBet {
    fn authorize(&self, username: &str, password: &str) -> Result<()> {
        let body = format!(r#"{{
            "BrowserFingerPrint": 1697977978,
            "Login": "{}",
            "Password": "{}",
            "RememberMe": true,
            "UserName": ""
        }}"#, username, password);

        let request = self.session.request("/api/authorization/post");
        let response = try!(request.post::<String, _>(body));

        if response.contains(r#""HasErrors":true"#) {
            Err(Error::from(response))
        } else {
            Ok(())
        }
    }

    fn check_balance(&self) -> Result<Currency> {
        let balance: Balance = try!(self.session.request("/api/account").get());

        Ok(Currency::from(balance.Balance))
    }

    fn watch(&self, cb: &Fn(Message)) -> Result<()> {
        // First of all, we should get initial page to get session cookie.
        try!(self.session.request("/").get::<String>());

        let mut timer = Periodic::new(3600);
        let polling_path = try!(self.generate_polling_path());

        loop {
            let mut state = try!(self.state.lock());

            if timer.next_if_elapsed() {
                let events = try!(self.get_events());

                state.odds_to_events_ids = HashMap::new();
                state.markets_to_events_ids = HashMap::new();

                for event in events {
                    try!(apply_event_update(&mut *state, event));
                }

                try!(provide_offers(&mut *state, cb));
            }

            let updates: PollingResponse = try!(self.session.request(&polling_path).get());

            try!(apply_updates(&mut *state, updates.M));
            try!(provide_offers(&mut *state, cb));
        }
    }

    fn place_bet(&self, offer: Offer, outcome: Outcome, stake: Currency) -> Result<()> {
        let state = &*try!(self.state.lock());
        let event = state.events.get(&(offer.oid as u32)).unwrap();
        let outcome_id = event.PreviewOdds.as_ref().unwrap().iter()
            .find(|o| o.Title == outcome.0)
            .unwrap().ID;

        let request_data = PlaceBetRequest {
            AcceptBetterOdds: true,
            Selections: vec![
                Bet {
                    Items: vec![
                        BetOutcome {
                            ID: outcome_id,
                            IsBanker: false
                        }
                    ],
                    Stake: stake.into(),
                    Return: (stake * outcome.1).into()
                }
            ]
        };

        let request = self.session.request("/api/betslip/place");
        let response: PlaceBetResponse = try!(request.post(request_data));

        match response.ErrorMessage {
            Some(m) => Err(Error::from(m)),
            None => Ok(())
        }
    }
}

#[derive(Debug)]
struct State {
    odds_to_events_ids: HashMap<u32, u32>,
    markets_to_events_ids: HashMap<u32, u32>,
    events: HashMap<u32, Event>,
    offers: HashMap<u32, Offer>,
    changed_events: HashSet<u32>
}

#[derive(Deserialize)]
struct Balance {
    Balance: f64
}

#[derive(Deserialize)]
struct Sport {
    ID: u32
}

#[derive(Deserialize, Debug)]
struct Event {
    ID: u32,
    IsSuspended: bool,
    DateOfMatch: String,

    PreviewOdds: Option<Vec<Odd>>,
    IsActive: Option<bool>,
    IsFinished: Option<bool>,
    SportType: Option<SportType>,
    Category: Option<Category>,
    PreviewMarket: Option<Market>
}

#[derive(Deserialize, Debug)]
struct Odd {
    ID: u32,
    IsSuspended: bool,
    IsVisible: bool,
    Value: f64,
    Title: String,
}

#[derive(Deserialize, Debug)]
struct SportType {
    Name: String
}

#[derive(Deserialize, Debug)]
struct Category {
    Name: String
}

#[derive(Deserialize, Debug)]
struct Market {
    ID: u32,
    Name: Option<String>,
    IsActive: bool,
    IsSuspended: bool
}

#[derive(Deserialize)]
struct PollingAuthResponse {
    ConnectionToken: String
}

#[derive(Deserialize)]
struct PollingResponse {
    M: Vec<PollingMessage>
}

enum PollingMessage {
    OddsUpdateMessage(OddsUpdateMessage),
    MatchesUpdateMessage(MatchesUpdateMessage),
    MarketsUpdateMessage(MarketsUpdateMessage),

    PrematchMatchesUpdateMessage(PrematchMatchesUpdateMessage),
    PrematchOddsUpdateMessage(PrematchOddsUpdateMessage),
    PrematchMarketsUpdateMessage(PrematchMarketsUpdateMessage),

    UnsupportedUpdate(UnsupportedUpdate)

}

impl Deserialize for PollingMessage {
    fn deserialize<D>(d: &mut D) -> StdResult<PM, D::Error> where D: Deserializer {
        let result: json::Value = try!(Deserialize::deserialize(d));

        if result.find("M").map_or(false, json::Value::is_string) {
            return Ok(PM::UnsupportedUpdate(UnsupportedUpdate("Even no M".to_string())));
        }

        let update_type = result.find("M").unwrap().as_str().unwrap_or("No update type").to_string();

        Ok(match update_type.as_ref() {
            "oddsUpdated" => PM::OddsUpdateMessage( json::from_value(result).unwrap() ),
            "marketsUpdated" => PM::MarketsUpdateMessage( json::from_value(result).unwrap() ),
            "matchesUpdated" => PM::MatchesUpdateMessage( json::from_value(result).unwrap() ),
            "prematchOddsUpdated" => PM::PrematchOddsUpdateMessage( json::from_value(result).unwrap() ),
            "prematchMarketsUpdated" => PM::PrematchMarketsUpdateMessage( json::from_value(result).unwrap() ),
            "prematchMatchesUpdated" => PM::PrematchMatchesUpdateMessage( json::from_value(result).unwrap() ),
            _ => PM::UnsupportedUpdate( UnsupportedUpdate(update_type))
        })
    }
}

#[derive(Deserialize)]
struct OddsUpdateMessage {
    A: Vec<Vec<OddUpdate>>
}

#[derive(Deserialize)]
struct MarketsUpdateMessage {
    A: Vec<Vec<Market>>
}

#[derive(Deserialize)]
struct MatchesUpdateMessage {
    A: Vec<Vec<Event>>
}

#[derive(Deserialize)]
struct PrematchOddsUpdateMessage {
    A: Vec<Vec<PrematchOddUpdate>>
}

#[derive(Deserialize)]
struct PrematchMarketsUpdateMessage {
    A: Vec<Vec<PrematchMarketUpdate>>
}

#[derive(Deserialize)]
struct PrematchMatchesUpdateMessage {
    A: Vec<Vec<PrematchMatchUpdate>>
}

#[derive(Deserialize)]
struct UnsupportedUpdate(String);

#[derive(Deserialize)]
struct OddUpdate {
    ID: u32,
    Value: f64,
    IsSuspended: bool,
    IsVisible: bool
}

#[derive(Deserialize)]
struct PrematchOddUpdate(u32, f64, i32);

#[derive(Deserialize)]
struct PrematchMarketUpdate(u32, i32);

#[derive(Deserialize)]
struct PrematchMatchUpdate(u32, i32, i64);

#[derive(Serialize, Debug)]
struct PlaceBetRequest {
    Selections: Vec<Bet>,
    AcceptBetterOdds: bool
}

#[derive(Serialize, Debug)]
struct Bet {
    Items: Vec<BetOutcome>,
    Stake: f64,
    Return: f64
}

#[derive(Serialize, Debug)]
struct BetOutcome {
    ID: u32,
    IsBanker: bool
}

#[derive(Deserialize, Debug)]
struct PlaceBetResponse {
    ErrorMessage: Option<String>,
}

fn convert_prematch_odd_update(update: &PrematchOddUpdate) -> OddUpdate {
    OddUpdate {
        ID: update.0,
        Value: update.1,
        IsSuspended: update.2 == 3, // IsSuspended status.
        IsVisible: update.2 == 1 || update.2 == 3 // Either active or suspended.
    }
}

fn convert_prematch_market_update(update: &PrematchMarketUpdate) -> Market {
    Market {
        ID: update.0,
        IsSuspended: update.1 == 3, // IsSuspended status.
        IsActive: update.1 == 1 || update.1 == 3, // Either active or suspended.
        Name: None
    }
}

fn convert_prematch_match_update(update: PrematchMatchUpdate) -> Event {
    let tm = time::at_utc(time::Timespec::new(update.2 as i64, 0));

    Event {
        ID: update.0,
        IsSuspended: update.1 == 3, // IsSuspended status.
        DateOfMatch: time::strftime("%Y-%m-%dT%H:%M:%S", &tm).unwrap(),

        IsFinished: None,
        PreviewOdds: None,
        IsActive: None,
        Category: None,
        SportType: None,
        PreviewMarket: None
    }
}

fn convert_event_into_offer(event: &Event) -> Result<Option<Offer>> {
    let game = get_game(&event);
    let kind = get_kind(&event);

    if event.IsSuspended
    || !event.IsActive.unwrap_or(true)
    || game.is_none() || kind.is_none() {
        return Ok(None);
    }


    match event.PreviewMarket {
        Some(ref market) => {
            if market.IsSuspended || !market.IsActive {
                return Ok(None);
            }

            match market.Name.as_ref().unwrap().trim() {
                "Match Odds" | "Match Winner" | "Match Odds (3 Way)" | "Series Winner" => {},
                _ => return Ok(None)
            }
        },
        None => return Ok(None)
    }

    match event.PreviewOdds {
        Some(ref odds) => {
            if odds.len() < 2 || odds.iter().any(|o| o.IsSuspended || !o.IsVisible) {
                return Ok(None);
            }
        },
        None => return Ok(None)
    }

    let odds = match event.PreviewOdds {
        Some(ref odds) => odds.iter()
            .map(|odd| {
                let title = if odd.Title == "Draw" { DRAW.to_owned() } else { odd.Title.clone() };

                Outcome(title, odd.Value)
            })
            .collect::<Vec<_>>(),
        None => return Ok(None)
    };

    let ts = try!(time::strptime(&event.DateOfMatch, "%Y-%m-%dT%H:%M:%S")).to_timespec();

    Ok(Some(Offer {
        oid: event.ID as OID,
        date: ts.sec as u32,
        game: game.unwrap(),
        kind: kind.unwrap(),
        outcomes: odds
    }))
}

fn get_game(event: &Event) -> Option<Game> {
    if event.Category.is_none() || event.SportType.is_none() {
        return None;
    }

    Some(match event.SportType.as_ref().unwrap().Name.as_str() {
        "eSports" => match event.Category.as_ref().unwrap().Name.as_str() {
            "League of Legends" => Game::LeagueOfLegends,
            "Heroes of the Storm" => Game::HeroesOfTheStorm,
            "Hearthstone" => Game::Hearthstone,
            "SMITE" => Game::Smite,
            "Counter-Strike: Global Offensive" => Game::CounterStrike,
            "Dota 2" => Game::Dota2,
            "Starcraft" => Game::StarCraft2,
            "Overwatch" => Game::Overwatch,
            "Halo" => Game::Halo,
            "World of Tanks" => Game::WorldOfTanks,
            // "CrossFire" => Game::CrossFire,
            "Vainglory" => Game::Vainglory,
            game => {
                warn!("New game in vitalbet eSports: {:?}", game);
                return None;
            }
        },
        "Soccer" | "Football" => Game::Football,
        "Basketball" => Game::Basketball,
        "Tennis" => Game::Tennis,
        "Table Tennis" => Game::TableTennis,
        "Volleyball" => Game::Volleyball,
        "Ice Hockey" => Game::IceHockey,
        "Handball" => Game::Handball,
        "Baseball" => Game::Baseball,
        "American Football" => Game::AmericanFootball,
        "Snooker" => Game::Snooker,
        "MMA" => Game::MartialArts,
        "Boxing" => Game::Boxing,
        "Formula 1" => Game::Formula,
        "Chess" => Game::Chess,
        game => {
            warn!("New game on vitalbet: {:?}", game);
            return None;
        }
    })
}

fn get_kind(event: &Event) -> Option<Kind> {
    Some(Kind::Series)
}

fn apply_updates(state: &mut State, messages: Vec<PollingMessage>) -> Result<()> {
    for msg in messages {
        match msg {
            PM::OddsUpdateMessage(ref msg) => {
                for odd_update in msg.A.iter().flat_map(|updates| updates.into_iter()) {
                    try!(apply_odd_update(state, odd_update));
                }
            },
            PM::PrematchOddsUpdateMessage(ref msg) => {
                for odd_update in msg.A.iter().flat_map(|updates| updates.iter()) {
                    try!(apply_odd_update(state, &convert_prematch_odd_update(odd_update),));
                }
            },
            PM::MarketsUpdateMessage(msg) => {
                for market_update in msg.A.into_iter().flat_map(|updates| updates.into_iter()) {
                    try!(apply_market_update(state, market_update));
                }
            },
            PM::PrematchMarketsUpdateMessage(msg) => {
                for market_update in msg.A.into_iter().flat_map(|updates| updates.into_iter()) {
                    try!(apply_market_update(state, convert_prematch_market_update(&market_update)));
                }
            },
            PM::MatchesUpdateMessage(msg) => {
                for event_update in msg.A.into_iter().flat_map(|updates| updates.into_iter()) {
                    try!(apply_event_update(state, event_update));
                }
            },
            PM::PrematchMatchesUpdateMessage(msg) => {
                for event_update in msg.A.into_iter().flat_map(|updates| updates.into_iter()) {
                    try!(apply_event_update(state, convert_prematch_match_update(event_update)));
                }
            },
            _ => {}
        }
    }

    Ok(())
}

fn apply_odd_update(state: &mut State, odd_update: &OddUpdate) -> Result<()> {
    if !state.odds_to_events_ids.contains_key(&odd_update.ID) {
        return Ok(());
    }

    let event_id = state.odds_to_events_ids[&odd_update.ID];

    if !state.events.contains_key(&event_id) {
        return Ok(());
    }

    let event = state.events.get_mut(&event_id).unwrap();

    // Find the odd we want to update and update it.
    if let Some(ref mut odds) = event.PreviewOdds {
        for odd in odds {
            if odd.ID == odd_update.ID {
                odd.Value = odd_update.Value;
                odd.IsSuspended = odd_update.IsSuspended;
                odd.IsVisible = odd_update.IsVisible;
            }
        }

        state.changed_events.insert(event.ID);
    }

    Ok(())
}

fn apply_market_update(state: &mut State, market_update: Market) -> Result<()> {
    if !state.markets_to_events_ids.contains_key(&market_update.ID) {
        return Ok(());
    }

    let event_id = state.markets_to_events_ids[&market_update.ID];

    if !state.events.contains_key(&event_id) {
        return Ok(());
    }

    let event = state.events.get_mut(&event_id).unwrap();

    if let Some(ref mut market) = event.PreviewMarket {
        market.IsSuspended = market_update.IsSuspended;
        market.IsActive = market_update.IsActive;

        state.changed_events.insert(event.ID);
    }

    Ok(())
}

fn apply_event_update(state: &mut State, event_update: Event) -> Result<()> {
    state.changed_events.insert(event_update.ID);

    if state.events.contains_key(&event_update.ID) {
        let event = state.events.get_mut(&event_update.ID).unwrap();

        event.IsSuspended = event_update.IsSuspended;
        event.DateOfMatch = event_update.DateOfMatch;

        if let Some(odds) = event_update.PreviewOdds {
            for odd in &odds {
                state.odds_to_events_ids.insert(odd.ID, event.ID);
            }

            event.PreviewOdds = Some(odds);
        }
    } else {
        if let Some(ref market) = event_update.PreviewMarket {
            state.markets_to_events_ids.insert(market.ID.clone(), event_update.ID.clone());
        }

        state.events.insert(event_update.ID, event_update);
    }

    Ok(())
}

fn provide_offers(state: &mut State, cb: &Fn(Message)) -> Result<()> {
    let mut upserted = 0;
    let mut removed = 0;

    for updated_event_id in state.changed_events.drain() {
        if let Some(offer) = try!(convert_event_into_offer(&state.events[&updated_event_id])) {
            state.offers.insert(offer.oid as u32, offer.clone());

            cb(Upsert(offer));

            upserted += 1;
        } else {
            if let Some(offer) = state.offers.remove(&updated_event_id) {
                cb(Remove(offer.oid));

                removed += 1;
            }

            if state.events[&updated_event_id].IsFinished.unwrap_or(false) {
                trace!("Event is finished: {:?}", state.events[&updated_event_id]);

                state.events.remove(&updated_event_id);
            }
        }
    }

    trace!("Upserted {} offers. Removed {} offers.", upserted, removed);

    Ok(())
}
