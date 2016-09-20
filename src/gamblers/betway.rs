#![allow(non_snake_case)]

use std::io::Read;
use std::collections::HashMap;
use kuchiki::{self, NodeRef};
use kuchiki::traits::TendrilSink;
use regex::Regex;
use serde_json as json;
use time;

use base::error::Result;
use base::timers::Periodic;
use base::parsing::{NodeRefExt, ElementDataExt};
use base::session::Session;
use base::currency::Currency;
use gamblers::Gambler;
use events::{Offer, Outcome, DRAW, Kind};
use events::kinds::*;

pub struct BetWay {
    session: Session
}

impl BetWay {
    pub fn new() -> BetWay {
        BetWay {
            session: Session::new("https://sports.betway.com")
        }
    }

    // TODO(universome): make this function generic over type?
    fn get_esports_events_ids(&self) -> Result<Vec<u32>> {
        // First we should get list of leagues
        let main_page = try!(self.session.get_html("/"));
        let events_types = try!(extract_events_types(main_page));
        let path = format!("/?u=/types/{}", events_types.join("+").to_owned());
        let response = try!(self.session.get_html(path.as_ref()));
        
        extract_events_ids(response)
    }

    fn get_events(&self, events_ids: Vec<u32>) -> Result<EventsResponse> {
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
        let mut provided_offers = HashMap::new();
        
        let initial_events_ids = try!(self.get_esports_events_ids());
        let events = try!(self.get_events(initial_events_ids)).result;

        for event in events {
            if let Some(offer) = try!(create_offer_from_event(&event)) {
                provided_offers.insert(offer.inner_id as u32, offer.clone());

                cb(offer, true);
            }
        }

        // TODO(universome): Run websockets for updates
        Ok(())
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
    eventIds: Vec<u32>,
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
    priceDec: f64,
    name: String,
    active: bool,
    typeCname: String
}

#[derive(Deserialize, Debug)]
struct Keyword {
    typeCname: String,
    cname: String
}

fn extract_ip_address(html_page: &String) -> Option<String> {
    let re = Regex::new(r#"config\["ip"] = "([\d|.]+)";"#).unwrap();

    re.captures(html_page)
        .and_then(|caps| caps.at(1)
            .and_then(|cap| Some(cap.to_string())))
}

fn extract_server_id(html_page: &String) -> Option<u32> {
    let re = Regex::new(r#"config\["serverId"] = (\d+);"#).unwrap();

    re.captures(html_page)
        .and_then(|caps| caps.at(1)
            .and_then(|cap| (cap.parse::<u32>()).ok()))
}

fn extract_client_type(html_page: &String) -> Option<u32> {
    let re = Regex::new(r#"clientType : (\d+),"#).unwrap();

    re.captures(html_page)
        .and_then(|caps| caps.at(1)
            .and_then(|cap| (cap.parse::<u32>()).ok() ))
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
    
    let events_ids = events_nodes
        .filter_map(|event_node| {
            let class = event_node.get_attr("class").unwrap();
            let event_id = &class[15..]; // "event_name evt_123" => "123"

            return match event_id.parse::<u32>() {
                Ok(event_id) => Some(event_id),
                Err(err) => {
                    warn!("Whoa, we have met some wierd class: {}, Erorr: {}", class, err);

                    None
                }
            }
        })
        .collect();

    Ok(events_ids)
}

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
                    warn!("New category on betway.com: {:?}", kind);

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

    Some(market.unwrap().outcomes.iter().map(|outcome| {
        // Converting "[NaVi]" into "NaVi".
        let title: String = outcome.name.chars()
            .skip(1)
            .take(outcome.name.len() - 2)
            .collect();

        let title = if title == "Draw" { DRAW.to_owned() } else { title };

        Outcome(title, outcome.priceDec)
    }).collect())
}
