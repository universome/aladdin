#![allow(non_snake_case)]

use std::collections::HashMap;
use std::result::Result as StdResult;
use kuchiki::NodeRef;
use regex::Regex;
use serde::{Deserialize, Deserializer};
use serde_json as json;
use time;

use base::error::Result;
use base::timers::Periodic;
use base::parsing::{NodeRefExt, ElementDataExt};
use base::session::Session;
use base::currency::Currency;
use base::websocket::Connection as Connection;
use gamblers::Gambler;
use events::{Offer, DRAW, Kind};
use events::Outcome as Outcome;
use events::kinds::*;

pub struct BetWay {
    session: Session
}

lazy_static! {
    static ref IP_ADDRESS_RE: Regex = Regex::new(r#"config\["ip"] = "([\d|.]+)";"#).unwrap();
    static ref SERVER_ID_RE: Regex = Regex::new(r#"config\["serverId"] = (\d+);"#).unwrap();
    static ref CLIENT_TYPE_RE: Regex = Regex::new(r#"clientType : (\d+),"#).unwrap();
}

impl BetWay {
    pub fn new() -> BetWay {
        BetWay {
            session: Session::new("sports.betway.com")
        }
    }

    fn get_esports_events_ids(&self) -> Result<Vec<u32>> {
        // First we should get list of leagues
        let main_page = try!(self.session.get_html("/"));
        let events_types = try!(extract_events_types(main_page));
        let path = format!("/?u=/types/{}", events_types.join("+"));
        let response = try!(self.session.get_html(path.as_ref()));

        extract_events_ids(response)
    }

    fn get_events(&self, events_ids: &Vec<u32>) -> Result<EventsResponse> {
        let path = "/emoapi/emos";
        let request_data = EventsRequestData {
            eventIds: events_ids,
            lang: "en"
        };

        let response = try!(self.session.post_json(path, request_data));

        Ok(try!(json::from_reader(response)))
    }
}

impl Gambler for BetWay {
    fn authorize(&self, username: &str, password: &str) -> Result<()> {
        let main_page = try!(self.session.get_raw_html("/"));

        let ip_address = try!(extract_ip_address(&main_page).ok_or("Could not extract ip_address"));
        let server_id = try!(extract_server_id(&main_page).ok_or("Could not extract server_id"));
        let client_type = try!(extract_client_type(&main_page).ok_or("Could not extract client_type"));

        let body = LoginRequestData {
            password: password,
            username: username,
            clientType: client_type,
            ipAddress: ip_address.as_ref(),
            serverId: server_id
        };

        self.session.post_json("/betapi/v4/login", body).map(|_| ())
    }

    fn check_balance(&self) -> Result<Currency> {
        let main_page = try!(self.session.get_raw_html("/"));
        let server_id = try!(extract_server_id(&main_page).ok_or("Could not extract server_id"));

        let request_data = BalanceRequestData {
            serverId: server_id,
            lang: "en",
            userId: 1
        };

        let response = try!(self.session.post_json("/betapi/v4/getCustomerInfo", request_data));
        let balance: Balance = try!(json::from_reader(response));

        Ok(Currency::from(balance.response.sbBalance))
    }

    fn watch(&self, cb: &Fn(Offer, bool)) -> Result<()> {
        let mut timer = Periodic::new(3600);
        let mut events = HashMap::new();
        let mut connection = try!(Connection::new("sports.betway.com/emoapi/push"));
        let mut is_inited = false;
        let session = self.session.get_cookie("SESSION").unwrap();

        loop {
            if !is_inited || timer.next_if_elapsed() {
                let events_ids = try!(self.get_esports_events_ids());
                let current_events = try!(self.get_events(&events_ids)).result;

                for event in current_events {
                    if events.contains_key(&event.eventId) {
                        continue;
                    }

                    if let Some(offer) = try!(create_offer_from_event(&event)) {
                        cb(offer, true);
                    }

                    let event_subscription = EventSubscription {
                        cmd: "eventSub",
                        session: &session,
                        eventIds: vec![event.eventId.clone()]
                    };

                    try!(connection.send(event_subscription));

                    events.insert(event.eventId, event);
                }

                is_inited = true;
            }

            let update = try!(connection.receive::<Update>());

            if let Some(mut event) = match update {
                Update::EventUpdate(ref u) => events.get_mut(&u.eventId),
                Update::MarketUpdate(ref u) => events.get_mut(&u.eventId),
                Update::OutcomeUpdate(ref u) => events.get_mut(&u.eventId),
                _ => None
            } {
                if !apply_update(&mut event, &update) {
                    warn!("We've somehow received an update for unknown event: {:?}", update);
                }

                if let Some(offer) = try!(create_offer_from_event(&event)) {
                    cb(offer, true);
                } else {
                    cb(try!(create_dummy_offer_from_event(&event)), false);
                }
            }
        }
    }

    fn place_bet(&self, offer: Offer, outcome: Outcome, bet: Currency) -> Result<()> {
        unimplemented!();
    }
}

#[derive(Serialize, Debug)]
struct LoginRequestData<'a> {
    username: &'a str,
    password: &'a str,
    clientType: u32,
    ipAddress: &'a str,
    serverId: u32
}

#[derive(Serialize, Debug)]
struct BalanceRequestData<'a> {
    userId: u32,
    lang: &'a str,
    serverId: u32
}

#[derive(Deserialize, Debug)]
struct Balance {
    response: BalanceResponse
}

#[derive(Deserialize, Debug)]
struct BalanceResponse {
    sbBalance: f64
}

#[derive(Serialize, Debug)]
struct EventsRequestData<'a> {
    eventIds: &'a Vec<u32>,
    lang: &'a str
}

#[derive(Deserialize, Debug)]
struct EventsResponse {
    result: Vec<Event>
}

#[derive(Deserialize, Debug)]
struct Event {
    eventId: u32,
    startAt: String,
    homeTeamCname: Option<String>,
    awayTeamCname: Option<String>,
    markets: Vec<Market>,
    keywords: Vec<Keyword>,
    active: bool,
    live: bool
}

#[derive(Deserialize, Debug)]
struct Market {
    marketId: u32,
    outcomes: Vec<BetwayOutcome>,
    active: bool,
    cname: String,
    typeCname: String
}

#[derive(Deserialize, Debug)]
struct BetwayOutcome {
    outcomeId: u32,
    priceDec: Option<f64>,
    name: String,
    active: bool,
    typeCname: String
}

#[derive(Deserialize, Debug)]
struct Keyword {
    typeCname: String,
    cname: String
}

#[derive(Serialize, Debug)]
struct EventSubscription<'a> {
    cmd: &'a str,
    session: &'a String,
    eventIds: Vec<u32>
}

#[derive(Debug)]
enum Update {
    EventUpdate(EventUpdate),
    MarketUpdate(MarketUpdate),
    OutcomeUpdate(OutcomeUpdate),
    UnsupportedUpdate(UnsupportedUpdate)
}

impl Deserialize for Update {
    fn deserialize<D>(d: &mut D) -> StdResult<Update, D::Error> where D: Deserializer {
        let result: json::Value = try!(Deserialize::deserialize(d));

        if !result.find("type").map_or(false, json::Value::is_string) {
            warn!("Type is absent in message: {}", result);
            return Ok(Update::UnsupportedUpdate(UnsupportedUpdate("Type is absent".to_string())));
        }

        let update_type = result.find("type").unwrap().as_str().unwrap_or("No update type").to_string();

        Ok(match update_type.as_ref() {
            "event" => Update::EventUpdate( json::from_value(result).unwrap() ),
            "market" => Update::MarketUpdate( json::from_value(result).unwrap() ),
            "outcome" => Update::OutcomeUpdate( json::from_value(result).unwrap() ),
            other_type => Update::UnsupportedUpdate( UnsupportedUpdate(other_type.to_string()))
        })
    }
}

#[derive(Deserialize, Debug)]
struct EventUpdate {
    eventId: u32,
    live: bool,
    active: Option<bool>,
    // started: bool
}

#[derive(Deserialize, Debug)]
struct MarketUpdate {
    eventId: u32,
    marketId: u32,
    active: Option<bool>
}

#[derive(Deserialize, Debug)]
struct OutcomeUpdate {
    eventId: u32,
    marketId: u32,
    outcomeId: u32,
    active: Option<bool>,
    priceDec: Option<f64>
}

#[derive(Deserialize, Debug)]
struct UnsupportedUpdate(String);

fn extract_ip_address(html_page: &String) -> Option<String> {
    IP_ADDRESS_RE.captures(html_page)
        .and_then(|caps| caps.at(1))
            .and_then(|cap| Some(cap.to_string()))
}

fn extract_server_id(html_page: &String) -> Option<u32> {
    SERVER_ID_RE.captures(html_page)
        .and_then(|caps| caps.at(1))
            .and_then(|cap| (cap.parse::<u32>()).ok())
}

fn extract_client_type(html_page: &String) -> Option<u32> {
    CLIENT_TYPE_RE.captures(html_page)
        .and_then(|caps| caps.at(1))
            .and_then(|cap| (cap.parse::<u32>()).ok())
}

// We do not return u32 (although we should), because we will have to convert String > u32 > String
fn extract_events_types(page: NodeRef) -> Result<Vec<String>> {
    let events_types = try!(page.query_all(".cb-esports"))
        .filter_map(|event_type_node| event_type_node.get_attr("id").ok())
        .collect();

    Ok(events_types)
}

fn extract_events_ids(page: NodeRef) -> Result<Vec<u32>> {
    let events_nodes = try!(page.query_all(".event_name"));
    let mut events_ids = Vec::new();

    for event_node in events_nodes {
        let classes = try!(event_node.get_attr("class"));
        // TODO(universome): why the fuck classes have to be mutable?
        let mut classes = classes.split_whitespace();
        let event_id: u32 = match classes.find(|c| c.starts_with("evt_")) {
            Some(c) => try!(c.trim_left_matches("evt_").parse()),
            None => continue
        };

        events_ids.push(event_id);
    }

    Ok(events_ids)
}

// TODO(universome): We should return vec of possible offers.
fn create_offer_from_event(event: &Event) -> Result<Option<Offer>> {
    let ts = try!(time::strptime(&event.startAt, "%Y-%m-%dT%H:%M:%SZ")).to_timespec();
    let kind = get_kind_from_event(event);
    let outcomes = get_outcomes_from_event(event);

    if kind.is_none() || outcomes.is_none() {
        return Ok(None);
    }

    Ok(Some(Offer {
        inner_id: event.eventId as u64,
        date: ts.sec as u32,
        kind: kind.unwrap(),
        outcomes: outcomes.unwrap()
    }))
}

fn create_dummy_offer_from_event(event: &Event) -> Result<Offer> {
    let ts = try!(time::strptime(&event.startAt, "%Y-%m-%dT%H:%M:%SZ")).to_timespec();
    let kind = get_kind_from_event(event);

    Ok(Offer {
        inner_id: event.eventId as u64,
        date: ts.sec as u32,
        kind: kind.unwrap(),
        outcomes: Vec::new()
    })
}

fn get_kind_from_event(event: &Event) -> Option<Kind> {
    for keyword in event.keywords.iter() {
        if keyword.typeCname == "country" {
            return Some(match keyword.cname.as_ref() {
                "cs-go" => Kind::CounterStrike(CounterStrike::Series),
                "dota-2" => Kind::Dota2(Dota2::Series),
                "league-of-legends" => Kind::LeagueOfLegends(LeagueOfLegends::Series),
                "hearthstone" => Kind::Hearthstone(Hearthstone::Series),
                "heroes-of-the-storm" => Kind::HeroesOfTheStorm(HeroesOfTheStorm::Series),
                "overwatch" => Kind::Overwatch(Overwatch::Series),
                "starcraft-2" => Kind::StarCraft2(StarCraft2::Series),
                "world-of-tanks" => Kind::WorldOfTanks(WorldOfTanks::Series),
                kind => {
                    warn!("Found new category: {:?}", kind);

                    return None;
                }
            })
        }
    }

    None
}

fn get_outcomes_from_event(event: &Event) -> Option<Vec<Outcome>> {
    let market = event.markets.iter()
        .find(|market| market.typeCname == "win-draw-win" || market.typeCname == "to-win");

    if market.is_none() {
        return None;
    }

    match market {
        Some(market) => {
            if market.outcomes.iter().any(|o| o.priceDec.is_none()) {
                return None;
            }
        },
        None => return None
    }

    Some(market.unwrap().outcomes.iter().map(|outcome| {
        // Converting "[NaVi]" into "NaVi".
        let title = outcome.name.trim_left_matches("[").trim_right_matches("]").to_string();
        let title = if title == "Draw" { DRAW.to_owned() } else { title };

        Outcome(title, outcome.priceDec.unwrap())
    }).collect())
}

fn apply_update(event: &mut Event, update: &Update) -> bool {
    match update {
        &Update::EventUpdate(ref u) => apply_event_update(event, u),
        &Update::MarketUpdate(ref u) => apply_market_update(event, u),
        &Update::OutcomeUpdate(ref u) => apply_outcome_update(event, u),
        _ => false
    }
}

fn apply_event_update(event: &mut Event, event_update: &EventUpdate) -> bool {
    event.active = event_update.active.unwrap_or(event.active);
    event.live = event_update.live;

    true
}

fn apply_market_update(event: &mut Event, market_update: &MarketUpdate) -> bool {
    let market = match event.markets.iter_mut().find(|m| m.marketId == market_update.marketId) {
        Some(m) => m,
        None => return false
    };

    market.active = market_update.active.unwrap_or(market.active);

    true
}

fn apply_outcome_update(event: &mut Event, outcome_update: &OutcomeUpdate) -> bool {
    let market = match event.markets.iter_mut().find(|m| m.marketId == outcome_update.marketId) {
        Some(m) => m,
        None => return false
    };

    let outcome = match market.outcomes.iter_mut().find(|o| o.outcomeId == outcome_update.outcomeId) {
        Some(o) => o,
        None => return false
    };

    outcome.priceDec = outcome_update.priceDec.or(outcome.priceDec);
    outcome.active = outcome_update.active.unwrap_or(outcome.active);

    true
}
