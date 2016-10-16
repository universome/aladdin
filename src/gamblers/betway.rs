#![allow(non_snake_case)]

use std::collections::HashMap;
use std::result::Result as StdResult;
use std::sync::Mutex;
use kuchiki::NodeRef;
use regex::Regex;
use serde::{Deserialize, Deserializer};
use serde_json as json;
use time;

use base::error::{Result, Error};
use base::timers::Periodic;
use base::parsing::{NodeRefExt, ElementDataExt};
use base::session::Session;
use base::currency::Currency;
use base::websocket::Connection as Connection;
use gamblers::Gambler;
use markets::{OID, Offer, Outcome, DRAW, Game, Kind};

pub struct BetWay {
    session: Session,
    state: Mutex<State>
}

lazy_static! {
    static ref IP_ADDRESS_RE: Regex = Regex::new(r#"config\["ip"] = "([\d|.]+)";"#).unwrap();
    static ref SERVER_ID_RE: Regex = Regex::new(r#"config\["serverId"] = (\d+);"#).unwrap();
    static ref CLIENT_TYPE_RE: Regex = Regex::new(r#"clientType : (\d+),"#).unwrap();
}

impl BetWay {
    pub fn new() -> BetWay {
        BetWay {
            session: Session::new("sports.betway.com"),
            state: Mutex::new(State {
                events: HashMap::new(),
                user_id: 0,
                server_id: 0
            })
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

    fn get_customer_info(&self) -> Result<CustomerInfoResponse> {
        let main_page = try!(self.session.get_raw_html("/"));
        let server_id = try!(extract_server_id(&main_page).ok_or("Could not extract server_id"));

        let request_data = CustomerInfoRequest {
            serverId: server_id,
            lang: "en",
            userId: 1
        };

        let response = try!(self.session.post_json("/betapi/v4/getCustomerInfo", request_data));
        let customer_info: CustomerInfo = try!(json::from_reader(response));

        Ok(customer_info.response)
    }

    fn set_user_state(&self) -> Result<()> {
        let mut state = try!(self.state.lock());
        let customer_info = try!(self.get_customer_info());

        state.user_id = customer_info.userId;
        state.server_id = customer_info.serverId;

        Ok(())
    }
}

impl Gambler for BetWay {
    fn authorize(&self, username: &str, password: &str) -> Result<()> {
        let main_page = try!(self.session.get_raw_html("/"));

        let server_id = try!(extract_server_id(&main_page).ok_or("Could not extract server_id"));
        let ip_address = try!(extract_ip_address(&main_page).ok_or("Could not extract ip_address"));
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
        let customer_info = try!(self.get_customer_info());

        Ok(Currency(customer_info.sbBalance))
    }

    fn watch(&self, cb: &Fn(Offer, bool)) -> Result<()> {
        try!(self.set_user_state());

        let mut timer = Periodic::new(3600);
        let mut connection = try!(Connection::new("sports.betway.com/emoapi/push"));
        let mut is_inited = false;
        let session = self.session.get_cookie("SESSION").unwrap();

        loop {
            let mut state = self.state.lock().unwrap();

            if !is_inited || timer.next_if_elapsed() {
                let events_ids = try!(self.get_esports_events_ids());
                let current_events = try!(self.get_events(&events_ids)).result;

                for event in current_events {
                    if state.events.contains_key(&event.eventId) {
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

                    state.events.insert(event.eventId, event);
                }

                is_inited = true;
            }

            let update = try!(connection.receive::<Update>());

            if let Some(mut event) = match update {
                Update::EventUpdate(ref u) => state.events.get_mut(&u.eventId),
                Update::MarketUpdate(ref u) => state.events.get_mut(&u.eventId),
                Update::OutcomeUpdate(ref u) => state.events.get_mut(&u.eventId),
                _ => None
            } {
                if apply_update(&mut event, &update) {
                    if let Some(offer) = try!(create_offer_from_event(&event)) {
                        cb(offer, true);
                    } else {
                        cb(try!(create_dummy_offer_from_event(&event)), false);
                    }
                }
            }
        }
    }

    fn place_bet(&self, offer: Offer, outcome: Outcome, stake: Currency) -> Result<()> {
        let state = self.state.lock().unwrap();
        let ref event = state.events.get(&(offer.oid as u32)).unwrap();
        let market_id = get_good_market_id(&event).unwrap();
        let market = event.markets.iter().find(|m| m.marketId == market_id).unwrap();
        let outcome = market.outcomes.iter().find(|o| o.get_title() == outcome.0).unwrap();

        let path = "/betapi/v4/initiateBets";
        let request_data = InitiateBetRequest {
            acceptPriceChange: 2,
            betPlacements: vec![
                BetPlacement {
                    numLines: 1,
                    selections: vec![
                        Bet {
                            priceType: 1,
                            eventId: event.eventId,
                            handicap: 0,
                            marketId: market.marketId,
                            subselections: vec![
                                BetOutcomeSelection {
                                    outcomeId: outcome.outcomeId
                                }
                            ],
                            priceNum: outcome.priceNum.unwrap(),
                            priceDen: outcome.priceDen.unwrap()
                        }
                    ],
                    stakePerLine: stake.0 as u32,
                    systemCname: "single",
                    useFreeBet: false,
                    eachWay: false
                }
            ],
            lang: "en",
            serverId: state.server_id,
            userId: state.user_id
        };

        let response = try!(self.session.post_json(path, request_data));
        let response: InitiateBetResponse = try!(json::from_reader(response));

        if !response.success || response.response.is_none() {
            return Err(Error::from(format!("Initiating bet failed: {:?}", response)));
        }

        let path = "/betapi/v4/lookupBets";
        let request_data = PlaceBetRequest {
            betRequestId: response.response.unwrap().betRequestId.unwrap(),
            userId: state.user_id,
            serverId: state.server_id
        };

        let response = try!(self.session.post_json(path, request_data));
        let response: PlaceBetResponse = try!(json::from_reader(response));

        if !response.success || response.error.is_some() {
            return Err(Error::from(format!("Placing bet failed: {:?}", response)));
        }

        Ok(())
    }
}

struct State {
    events: HashMap<u32, Event>,
    user_id: u32,
    server_id: u32
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
struct CustomerInfoRequest<'a> {
    userId: u32,
    lang: &'a str,
    serverId: u32
}

#[derive(Deserialize, Debug)]
struct CustomerInfo {
    response: CustomerInfoResponse
}

#[derive(Deserialize, Debug)]
struct CustomerInfoResponse {
    sbBalance: i64,
    userId: u32,
    serverId: u32
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
    typeCname: String,
    displayed: bool
}

#[derive(Deserialize, Debug)]
struct BetwayOutcome {
    outcomeId: u32,
    priceDec: Option<f64>,
    priceNum: Option<u32>,
    priceDen: Option<u32>,
    name: String,
    active: bool,
    typeCname: String
}

impl BetwayOutcome {
    fn get_title(&self) -> String {
        self.name.trim_left_matches("[").trim_right_matches("]").to_string()
    }
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
            "event" | "gameEvent" => Update::EventUpdate( json::from_value(result).unwrap() ),
            "market" => Update::MarketUpdate( json::from_value(result).unwrap() ),
            "outcome" => Update::OutcomeUpdate( json::from_value(result).unwrap() ),
            other_type => Update::UnsupportedUpdate( UnsupportedUpdate(other_type.to_string()))
        })
    }
}

#[derive(Deserialize, Debug)]
struct EventUpdate {
    eventId: u32,
    live: Option<bool>,
    active: Option<bool>,
    // started: bool
}

#[derive(Deserialize, Debug)]
struct MarketUpdate {
    eventId: u32,
    marketId: u32,
    active: Option<bool>,
    displayed: Option<bool>
}

#[derive(Deserialize, Debug)]
struct OutcomeUpdate {
    eventId: u32,
    marketId: u32,
    outcomeId: u32,
    active: Option<bool>,
    priceDec: Option<f64>,
    priceNum: Option<u32>,
    priceDen: Option<u32>
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
    let game_and_kind = get_game_and_kind(event);
    let outcomes = get_outcomes_from_event(event);

    if game_and_kind.is_none() || outcomes.is_none() || !event.active {
        return Ok(None);
    }

    let (game, kind) = game_and_kind.unwrap();

    Ok(Some(Offer {
        oid: event.eventId as OID,
        date: ts.sec as u32,
        game: game,
        kind: kind,
        outcomes: outcomes.unwrap()
    }))
}

fn create_dummy_offer_from_event(event: &Event) -> Result<Offer> {
    let ts = try!(time::strptime(&event.startAt, "%Y-%m-%dT%H:%M:%SZ")).to_timespec();
    let (game, kind) = get_game_and_kind(event).unwrap();

    Ok(Offer {
        oid: event.eventId as OID,
        date: ts.sec as u32,
        game: game,
        kind: kind,
        outcomes: Vec::new()
    })
}

fn get_game_and_kind(event: &Event) -> Option<(Game, Kind)> {
    event.keywords.iter()
        .find(|kw| kw.typeCname == "country")
        .and_then(|kw| Some((match kw.cname.as_ref() {
            "cs-go" => Game::CounterStrike,
            "dota-2" => Game::Dota2,
            "league-of-legends" => Game::LeagueOfLegends,
            "hearthstone" => Game::Hearthstone,
            "heroes-of-the-storm" => Game::HeroesOfTheStorm,
            "overwatch" => Game::Overwatch,
            "starcraft-2" => Game::StarCraft2,
            "world-of-tanks" => Game::WorldOfTanks,
            game => {
                warn!("Found new category: {:?}", game);
                return None;
            }
        }, Kind::Series)))
}

fn get_good_market_id(event: &Event) -> Option<u32> {
    let markets = event.markets.iter()
        .filter(|m| m.active && m.displayed)
        .filter(|m| m.cname == "match-winner" || m.cname == "win-draw-win")
        .collect::<Vec<_>>();

    let market = match markets.len() {
        0 => return None,
        1 => markets[0],
        _ => {
            warn!("Can't choose market from event: {:?}", event);

            return None;
        }
    };

    if market.outcomes.iter().any(|o| o.priceDec.is_none()) {
        return None;
    }

    Some(market.marketId)
}

fn get_outcomes_from_event(event: &Event) -> Option<Vec<Outcome>> {
    let market = match get_good_market_id(event) {
        Some(id) => event.markets.iter().find(|m| m.marketId == id).unwrap(),
        None => return None
    };

    Some(market.outcomes.iter().map(|outcome| {
        // Converting "[NaVi]" into "NaVi".
        let name = outcome.get_title();
        let title = if name == "Draw" { DRAW.to_owned() } else { name };

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

fn apply_event_update(event: &mut Event, update: &EventUpdate) -> bool {
    let mut is_updated = false;

    if update.active.is_some() && update.active.unwrap() != event.active {
        event.active = update.active.unwrap();
        is_updated = true;
    }

    if update.live.is_some() && update.live.unwrap() != event.live {
        event.live = update.live.unwrap();
        is_updated = true;
    }

    is_updated
}

fn apply_market_update(event: &mut Event, update: &MarketUpdate) -> bool {
    let mut is_updated = false;

    let market = match event.markets.iter_mut().find(|m| m.marketId == update.marketId) {
        Some(m) => m,
        None => return is_updated
    };

    if update.active.is_some() && update.active.unwrap() != market.active {
        market.active = update.active.unwrap();
        is_updated = true;
    }

    if update.displayed.is_some() && update.displayed.unwrap() != market.displayed {
        market.displayed = update.displayed.unwrap();
        is_updated = true;
    }

    is_updated
}

fn apply_outcome_update(event: &mut Event, update: &OutcomeUpdate) -> bool {
    let mut is_updated = false;

    let market = match event.markets.iter_mut().find(|m| m.marketId == update.marketId) {
        Some(m) => m,
        None => return is_updated
    };

    let outcome = match market.outcomes.iter_mut().find(|o| o.outcomeId == update.outcomeId) {
        Some(o) => o,
        None => return is_updated
    };

    if update.priceDec.is_some() && update.priceDec != outcome.priceDec {
        outcome.priceDec = update.priceDec;
        is_updated = true;
    }

    if update.priceDen.is_some() && update.priceDen != outcome.priceDen {
        outcome.priceDen = update.priceDen;
        is_updated = true;
    }

    if update.priceNum.is_some() && update.priceNum != outcome.priceNum {
        outcome.priceNum = update.priceNum;
        is_updated = true;
    }

    if update.active.is_some() && update.active != Some(outcome.active) {
        outcome.active = update.active.unwrap();
        is_updated = true;
    }

    is_updated
}

#[derive(Serialize, Debug)]
struct InitiateBetRequest<'a> {
    acceptPriceChange: u32,
    betPlacements: Vec<BetPlacement<'a>>,
    lang: &'a str,
    serverId: u32,
    userId: u32
}

#[derive(Serialize, Debug)]
struct BetPlacement<'a> {
    numLines: u32,
    selections: Vec<Bet>,
    stakePerLine: u32,
    systemCname: &'a str,
    useFreeBet: bool,
    eachWay: bool
}

#[derive(Serialize, Debug)]
struct Bet {
    priceType: u32,
    eventId: u32,
    handicap: u32,
    marketId: u32,
    subselections: Vec<BetOutcomeSelection>,
    priceNum: u32,
    priceDen: u32
}

#[derive(Serialize, Debug)]
struct BetOutcomeSelection {
    outcomeId: u32
}

#[derive(Deserialize, Debug)]
struct InitiateBetResponse {
    success: bool,
    response: Option<InitiateBetResponseData>
}

#[derive(Deserialize, Debug)]
struct InitiateBetResponseData {
    betRequestId: Option<String>
}

#[derive(Serialize, Debug)]
struct PlaceBetRequest {
    betRequestId: String,
    userId: u32,
    serverId: u32
}

#[derive(Deserialize, Debug)]
struct PlaceBetResponse {
    success: bool,
    error: Option<PlaceBetError>
}

#[derive(Deserialize, Debug)]
struct PlaceBetError {
    message: String,
    details: Option<Vec<PlaceBetErrorDetail>>
}

#[derive(Deserialize, Debug)]
struct PlaceBetErrorDetail {
    min: u32,
    max: u32
}
