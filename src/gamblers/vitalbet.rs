#![allow(non_snake_case)]

use std::result::Result as StdResult;
use std::collections::HashMap;
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
                odds_to_events: HashMap::new(),
                markets_to_events: HashMap::new(),
                events: HashMap::new()
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
                state.odds_to_events = HashMap::new();
                state.markets_to_events = HashMap::new();

                let current_events = try!(self.get_events());

                for event in current_events {
                    if let Some(offer) = try!(create_offer(&event)) {
                        cb(Upsert(offer));
                    }

                    // Save data into state.
                    if let Some(ref odds) = event.PreviewOdds {
                        for odd in odds {
                            state.odds_to_events.insert(odd.ID, event.ID);
                        }
                    };

                    if let Some(ref market) = event.PreviewMarket {
                        state.markets_to_events.insert(market.ID, event.ID);
                    };

                    state.events.insert(event.ID, event);
                }
            }

            let messages = try!(self.session.request(&polling_path).get::<PollingResponse>());
            let updates = flatten_updates(messages.M);

            for update in updates {
                if let Some(mut event) = find_event_for_update(&mut state, &update) {
                    if apply_update(&mut event, &update) {
                        if let Some(offer) = try!(create_offer(&event)) {
                            cb(Upsert(offer));
                        } else {
                            cb(Remove(event.ID as OID));
                        }
                    }
                }
            }
        }
    }

    fn place_bet(&self, offer: Offer, outcome: Outcome, stake: Currency) -> Result<()> {
        let state = &*try!(self.state.lock());
        let event = &state.events[&(offer.oid as u32)];
        let outcome_id = event.PreviewOdds.as_ref().unwrap().iter()
            .find(|o| o.Title == outcome.0 || (outcome.0 == DRAW && o.Title == "Draw"))
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
    odds_to_events: HashMap<u32, u32>,
    markets_to_events: HashMap<u32, u32>,
    events: HashMap<u32, Event>
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

    UnsupportedUpdateMessage(UnsupportedUpdateMessage)
}

impl Deserialize for PollingMessage {
    fn deserialize<D>(d: &mut D) -> StdResult<PM, D::Error> where D: Deserializer {
        let result: json::Value = try!(Deserialize::deserialize(d));

        if result.find("M").map_or(false, json::Value::is_string) {
            return Ok(PM::UnsupportedUpdateMessage(UnsupportedUpdateMessage("Even no M".to_string())));
        }

        let update_type = result.find("M").unwrap().as_str().unwrap_or("No update type").to_string();

        Ok(match update_type.as_ref() {
            "oddsUpdated" => PM::OddsUpdateMessage( json::from_value(result).unwrap() ),
            "marketsUpdated" => PM::MarketsUpdateMessage( json::from_value(result).unwrap() ),
            "matchesUpdated" => PM::MatchesUpdateMessage( json::from_value(result).unwrap() ),
            "prematchOddsUpdated" => PM::PrematchOddsUpdateMessage( json::from_value(result).unwrap() ),
            "prematchMarketsUpdated" => PM::PrematchMarketsUpdateMessage( json::from_value(result).unwrap() ),
            "prematchMatchesUpdated" => PM::PrematchMatchesUpdateMessage( json::from_value(result).unwrap() ),
            _ => PM::UnsupportedUpdateMessage( UnsupportedUpdateMessage(update_type) )
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
struct UnsupportedUpdateMessage(String);

#[derive(Deserialize)]
enum Update {
    OddUpdate(OddUpdate),
    Market(Market),
    Event(Event),
    PrematchOddUpdate(PrematchOddUpdate),
    PrematchMarketUpdate(PrematchMarketUpdate),
    PrematchMatchUpdate(PrematchMatchUpdate)
}

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

fn convert_prematch_match_update(update: &PrematchMatchUpdate) -> Event {
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

fn create_offer(event: &Event) -> Result<Option<Offer>> {
    let game = get_game(&event);
    let kind = get_kind(&event);

    if event.IsSuspended || event.IsFinished.unwrap_or(false)
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
            "Gears of War" => Game::GearsOfWar,
            "Starcraft" => Game::StarCraft2,
            "Overwatch" => Game::Overwatch,
            "Halo" => Game::Halo,
            "World of Tanks" => Game::WorldOfTanks,
            "Vainglory" => Game::Vainglory,
            game => {
                warn!("New game in vitalbet eSports: {:?}", game);
                return None;
            }
        },
        "Alpine Skiing" => Game::AlpineSkiing,
        "American Football" => Game::AmericanFootball,
        "Bandy" => Game::Bandy,
        "Badminton" => Game::Badminton,
        "Baseball" => Game::Baseball,
        "Darts" => Game::Darts,
        "Basketball" => Game::Basketball,
        "Boxing" => Game::Boxing,
        "Chess" => Game::Chess,
        "Formula 1" => Game::Formula,
        "Golf" => Game::Golf,
        "Handball" => Game::Handball,
        "Ice Hockey" => Game::IceHockey,
        "MMA" => Game::MartialArts,
        "Rugby Union" => Game::Rugby,
        "Ski Jumping" => Game::SkiJumping,
        "Snooker" => Game::Snooker,
        "Soccer" | "Football" => Game::Football,
        "Table Tennis" => Game::TableTennis,
        "Tennis" => Game::Tennis,
        "Volleyball" => Game::Volleyball,
        game => {
            warn!("New game on vitalbet: {:?}", game);
            return None;
        }
    })
}

fn get_kind(event: &Event) -> Option<Kind> {
    Some(Kind::Series)
}

// XXX(universome)
// We recieve updates from vitalbet in a very nested format.
// So we would like to transform them into a normal flat vector.
fn flatten_updates(messages: Vec<PollingMessage>) -> Vec<Update> {
    let mut updates = Vec::new();

    for msg in messages {
        match msg {
            PM::OddsUpdateMessage(m) =>
                updates.extend(m.A.into_iter().flat_map(|v| v).map(Update::OddUpdate)),
            PM::MarketsUpdateMessage(m) =>
                updates.extend(m.A.into_iter().flat_map(|v| v).map(Update::Market)),
            PM::MatchesUpdateMessage(m) =>
                updates.extend(m.A.into_iter().flat_map(|v| v).map(Update::Event)),

            PM::PrematchOddsUpdateMessage(m) =>
                updates.extend(m.A.into_iter().flat_map(|v| v).map(Update::PrematchOddUpdate)),
            PM::PrematchMarketsUpdateMessage(m) =>
                updates.extend(m.A.into_iter().flat_map(|v| v).map(Update::PrematchMarketUpdate)),
            PM::PrematchMatchesUpdateMessage(m) =>
                updates.extend(m.A.into_iter().flat_map(|v| v).map(Update::PrematchMatchUpdate)),

            _ => {}
        }
    }

    updates
}

fn find_event_for_update<'a>(state: &'a mut State, update: &Update) -> Option<&'a mut Event> {
    let event_id = match update {
        &Update::OddUpdate(ref u) => state.odds_to_events.get(&u.ID),
        &Update::Market(ref u) => state.markets_to_events.get(&u.ID),
        &Update::Event(ref u) => Some(&u.ID),
        &Update::PrematchOddUpdate(ref u) => Some(&u.0),
        &Update::PrematchMarketUpdate(ref u) => state.markets_to_events.get(&u.0),
        &Update::PrematchMatchUpdate(ref u) => Some(&u.0),
    };

    match event_id {
        Some(eid) => state.events.get_mut(&eid),
        None => None
    }
}

fn apply_update(event: &mut Event, update: &Update) -> bool {
    match update {
        &Update::OddUpdate(ref u) => apply_odd_update(event, u),
        &Update::Market(ref u) => apply_market_update(event, u),
        &Update::Event(ref u) => apply_event_update(event, u),
        &Update::PrematchOddUpdate(ref u) => apply_odd_update(event, &convert_prematch_odd_update(u)),
        &Update::PrematchMarketUpdate(ref u) => apply_market_update(event, &convert_prematch_market_update(u)),
        &Update::PrematchMatchUpdate(ref u) => apply_event_update(event, &convert_prematch_match_update(u))
    }
}

fn apply_odd_update(event: &mut Event, odd_update: &OddUpdate) -> bool {
    if let Some(ref mut odds) = event.PreviewOdds {
        if let Some(ref mut odd) = odds.iter_mut().find(|odd| odd.ID == odd_update.ID) {
            odd.Value = odd_update.Value;
            odd.IsSuspended = odd_update.IsSuspended;
            odd.IsVisible = odd_update.IsVisible;

            return true;
        }
    }

    false
}

fn apply_market_update(event: &mut Event, market_update: &Market) -> bool {
    if let Some(ref mut market) = event.PreviewMarket {
        market.IsSuspended = market_update.IsSuspended;
        market.IsActive = market_update.IsActive;

        return true;
    }

    false
}

fn apply_event_update(event: &mut Event, event_update: &Event) -> bool {
    event.IsSuspended = event_update.IsSuspended;
    event.IsActive = event_update.IsActive;
    event.IsFinished = event_update.IsFinished.or(event.IsFinished);

    true
}
