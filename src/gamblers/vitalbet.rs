#![allow(non_snake_case)]

use time;
use base::error::Result;
use std::result::Result as stdResult;
use std::collections::{HashMap, HashSet};
use url::percent_encoding::{utf8_percent_encode, USERINFO_ENCODE_SET};
use serde::{Deserialize, Deserializer, de};
use serde_json;

use base::timers::Periodic;
use base::session::Session;
use base::currency::Currency;
use gamblers::Gambler;
use events::{Offer, Outcome, Kind};
use events::kinds::*;

use self::PollingMessage as PM;

pub struct VitalBet {
    session: Session
}

define_encode_set! {
    // TODO(universome): Is it possible to make it not public?
    pub VITALBET_ENCODE_SET = [USERINFO_ENCODE_SET] | {'+', '-'}
}

impl VitalBet {
    pub fn new() -> VitalBet {
        VitalBet {
            session: Session::new("https://vitalbet.com")
        }
    }

    // TODO(universome): pass timestamps, like they do
    fn generate_polling_path(&self) -> String {
        // First, we should get connection ConnectionToken
        let auth_path = concat!("/signalr/negotiate?transport=longPolling&clientProtocol=1.5",
                                "&connectionData=%5B%7B%22name%22%3A%22sporttypehub%22%7D%5D");
        let response = self.session.get_json::<PollingAuthResponse>(auth_path);
        let ConnectionToken = response.unwrap().ConnectionToken;
        let ConnectionToken = utf8_percent_encode(&ConnectionToken, VITALBET_ENCODE_SET).collect::<String>();

        // We should notify them, that we are starting polling (because they do it too)
        self.session.get_raw_json(&format!(concat!("/signalr/start?transport=longPolling",
                                 "&connectionData=%5B%7B%22name%22%3A%22sporttypehub%22%7D%5D",
                                 "clientProtocol=1.5&connectionToken={}"), ConnectionToken));

        format!(concat!("/signalr/poll?transport=longPolling&clientProtocol=1.5",
                        "&connectionData=%5B%7B%22name%22%3A%22sporttypehub%22%7D%5D",
                        "&connectionToken={}"), ConnectionToken)
    }
}

impl Gambler for VitalBet {
    fn authorize(&self, username: &str, password: &str) -> Result<()> {
        let body = AuthData {
            BrowserFingerPrint: 426682306,
            Login: username,
            Password: password,
            RememberMe: true
        };

        self.session.post_json("/api/authorization/post", body).map(|_| ())
    }

    fn check_balance(&self) -> Result<Currency> {
        let balance = try!(self.session.get_json::<Balance>("/api/account"));
        let money = balance.Balance;

        Ok(Currency::from(money))
    }

    fn watch(&self, cb: &Fn(Offer, bool)) -> Result<()> {
        let mut state = State {
            matches: HashMap::new(),
            odds_to_matches_ids: HashMap::new(),
            offers: HashMap::new(),
            changed_matches: HashSet::new()
        };

        // First of all, we should get initial page to get session cookie
        try!(self.session.get_html("/"));

        // Now we have some cookie! Let's get initial offers
        // TODO(universome): we should get offers from other sports too, not only Dota 2.
        let path = "/api/sportmatch/Get?sportID=2357";
        let initial_matches = try!(self.session.get_json::<Vec<Match>>(path));

        for match_ in initial_matches {
            if let Some(offer) = try!(convert_match_into_offer(&match_)) {
                state.offers.insert(offer.inner_id as u32, offer.clone());

                cb(offer, true);

                if let Some(ref odds) = match_.odds {
                    for odd in odds {
                        state.odds_to_matches_ids.insert(odd.ID, match_.ID);
                    }
                }

                state.matches.insert(match_.ID, match_);
            }
        }

        let polling_path = self.generate_polling_path();

        loop {
            let updates: PollingResponse = try!(self.session.get_json(&polling_path));
            
            apply_updates(&mut state, updates.M);

            provide_offers(&mut state, cb);
        }

        Ok(())
    }

    fn place_bet(&self, offer: Offer, outcome: Outcome, bet: Currency) -> Result<()> {
        unimplemented!();
    }
}

#[derive(Debug)]
struct State {
    odds_to_matches_ids: HashMap<u32, u32>,
    matches: HashMap<u32, Match>,
    offers: HashMap<u32, Offer>,
    changed_matches: HashSet<u32>
}

#[derive(Serialize)]
struct AuthData<'a> {
    BrowserFingerPrint: i64,
    Login: &'a str,
    Password: &'a str,
    RememberMe: bool
}

#[derive(Deserialize)]
struct Balance {
    Balance: f64
}

#[derive(Deserialize, Debug)]
struct Match {
    ID: u32,
    IsSuspended: bool,
    DateOfMatch: String,
    
    #[serde(rename = "PreviewOdds")]
    odds: Option<Vec<Odd>>,
    IsActive: Option<bool>,
    IsFinished: Option<bool>,
    Category: Option<Category>
}

#[derive(Deserialize, Debug)]
struct Odd {
    ID: u32,
    IsSuspended: bool,
    Value: f64,
    Title: String,
}

#[derive(Deserialize, Debug)]
struct Category {
    ID: u32,
    Name: String
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
    PrematchOddsUpdateMessage(PrematchOddsUpdateMessage),
    MatchesUpdateMessage(MatchesUpdateMessage),
    PrematchMatchesUpdateMessage(PrematchMatchesUpdateMessage),
    UnsupportedUpdate(UnsupportedUpdate),
}

impl Deserialize for PollingMessage {
    fn deserialize<D>(d: &mut D) -> stdResult<PM, D::Error> where D: Deserializer {
        let result: serde_json::Value = try!(Deserialize::deserialize(d));

        if result.find("M").is_none() {
            return Ok(PM::UnsupportedUpdate(UnsupportedUpdate("even no M".to_string())));
        }

        if result.find("M").unwrap().as_str().is_none() {
            return Ok(PM::UnsupportedUpdate(UnsupportedUpdate("even no M".to_string())));
        }

        let update_type = result.find("M").unwrap().as_str().unwrap().to_string();

        Ok(match update_type.as_ref() {
            "oddsUpdated" => PM::OddsUpdateMessage( serde_json::from_value(result).unwrap() ),
            "prematchOddsUpdated" => PM::PrematchOddsUpdateMessage( serde_json::from_value(result).unwrap() ),
            "matchesUpdated" => PM::MatchesUpdateMessage( serde_json::from_value(result).unwrap() ),
            "prematchMatchesUpdated" => PM::PrematchMatchesUpdateMessage( serde_json::from_value(result).unwrap() ),
            _ => PM::UnsupportedUpdate( UnsupportedUpdate(update_type.to_string()))
        })
    }
}

#[derive(Deserialize)]
struct OddsUpdateMessage {
    A: Vec<Vec<OddUpdate>>
}

#[derive(Deserialize)]
struct PrematchOddsUpdateMessage {
    A: Vec<Vec<PrematchOddUpdate>>
}

#[derive(Deserialize)]
struct PrematchMatchesUpdateMessage {
    A: Vec<Vec<PrematchMatchUpdate>>
}

#[derive(Deserialize)]
struct MatchesUpdateMessage {
    A: Vec<Vec<Match>>
}

#[derive(Deserialize)]
struct UnsupportedUpdate(String);

#[derive(Deserialize)]
struct OddUpdate {
    ID: u32,
    Value: f64,
    IsSuspended: bool
}

#[derive(Deserialize)]
struct PrematchOddUpdate(u32, f64, i32);

fn convert_prematch_odd_update(update: &PrematchOddUpdate) -> OddUpdate {
    OddUpdate {
        ID: update.0,
        Value: update.1,
        IsSuspended: update.2 == 3 // IsSuspended status (look at their js)
    }
}

#[derive(Deserialize)]
struct PrematchMatchUpdate(u32, i32, i64);

fn convert_prematch_match_update(update: PrematchMatchUpdate) -> Match {
    let tm = time::at_utc(time::Timespec::new(update.2 as i64, 0));

    Match {
        ID: update.0,
        IsSuspended: update.1 == 3, // IsSuspended status (look at their js)
        DateOfMatch: time::strftime("%Y-%m-%dT%H:%M:%S", &tm).unwrap(),

        IsFinished: None,
        odds: None,
        IsActive: None,
        Category: None
    }
}

fn convert_match_into_offer(match_: &Match) -> Result<Option<Offer>> {
    let kind = get_kind_from_match(&match_);

    if match_.IsSuspended || !match_.IsActive.unwrap_or(true) || kind.is_none() {
        return Ok(None);
    }

    let odds = match match_.odds {
        Some(ref odds) =>
            odds.iter()
                .filter(|odd| !odd.IsSuspended)
                .map(|odd| Outcome(odd.Title.clone(), odd.Value))
                .collect::<Vec<_>>(),
        None => return Ok(None)
    };

    if odds.len() == 0 {
        return Ok(None);
    }

    let ts = try!(time::strptime(&match_.DateOfMatch, "%Y-%m-%dT%H:%M:%S")).to_timespec();

    Ok(Some(Offer {
        date: ts.sec as u32,
        kind: kind.unwrap(),
        outcomes: odds,
        inner_id: match_.ID as u64
    }))
}

fn get_kind_from_match(match_: &Match) -> Option<Kind> {
    if match_.Category.is_none() {
        return None;
    }

    // TODO(universome): find out new kinds
    match match_.Category.as_ref().unwrap().ID {
        3683 => Some(Kind::CounterStrike(CounterStrike::Series)),
        3693 => Some(Kind::Dota2(Dota2::Series)),
        5791 => Some(Kind::Overwatch(Overwatch::Series)),
        3578 => Some(Kind::LeagueOfLegends(LeagueOfLegends::Series)),
        3600 => Some(Kind::Smite(Smite::Series)),
        3704 => Some(Kind::StarCraft2(StarCraft2::Series)),
        3601 => Some(Kind::WorldOfTanks(WorldOfTanks::Series)),
        _ => {
            debug!("New category in vitalbet esports: {:?}", match_.Category);
            None
        }
    }
}

fn apply_updates(state: &mut State, messages: Vec<PollingMessage>) {
    for msg in messages.into_iter() {
        match msg {
            PM::OddsUpdateMessage(ref msg) => {
                for odd_update in msg.A.iter().flat_map(|updates| updates.into_iter()) {
                    apply_odd_update(odd_update, state);
                }
            },
            PM::PrematchOddsUpdateMessage(ref msg) => {
                for odd_update in msg.A.iter().flat_map(|updates| updates.iter()) {
                    apply_odd_update(&convert_prematch_odd_update(odd_update), state);
                }
            },
            PM::MatchesUpdateMessage(msg) => {
                for match_update in msg.A.into_iter().flat_map(|updates| updates.into_iter()) {
                    apply_match_update(match_update, state);
                }
            },
            PM::PrematchMatchesUpdateMessage(msg) => {
                for match_update in msg.A.into_iter().flat_map(|updates| updates.into_iter()) {
                    apply_match_update(convert_prematch_match_update(match_update), state);
                }
            },
            _ => {}
        }
    }
}

fn apply_odd_update(odd_update: &OddUpdate, state: &mut State) -> Result<()> {
    if !state.odds_to_matches_ids.contains_key(&odd_update.ID) {
        // This is an update for some odd, which we do not track
        return Ok(());
    }

    let match_id = state.odds_to_matches_ids.get(&odd_update.ID).unwrap();

    if !state.matches.contains_key(&match_id) {
        // This is an update for some match, which we do not track
        return Ok(());
    }

    let match_ = state.matches.get_mut(&match_id).unwrap();

    // Find the odd we want to update and update it
    if let Some(ref mut odds) = match_.odds {
        for odd in odds {
            if odd.ID == odd_update.ID {
                odd.Value = odd_update.Value;
                odd.IsSuspended = odd_update.IsSuspended;
            }
        }

        if !state.changed_matches.contains(&match_.ID) {
            state.changed_matches.insert(match_.ID);
        }
    } else {
        unreachable!(); // For debug purposes
    }

    Ok(())
}

fn apply_match_update(match_update: Match, state: &mut State) -> Result<()> {
    state.changed_matches.insert(match_update.ID);

    if state.matches.contains_key(&match_update.ID) {
        let match_ = state.matches.get_mut(&match_update.ID).unwrap();

        match_.IsSuspended = match_update.IsSuspended;
        match_.DateOfMatch = match_update.DateOfMatch;

        if let Some(odds) = match_update.odds {
            debug!("We have odds matchesUpdated (!!!): {:?}", odds);
            match_.odds = Some(odds);
        }
    } else {
        state.matches.insert(match_update.ID, match_update);
    }

    Ok(())
}

fn provide_offers(state: &mut State, cb: &Fn(Offer, bool)) -> Result<()> {
    // TODO(universome): detect changes more accurately
    for updated_match_id in state.changed_matches.drain() {
        // First of all we should remove our old offer
        if state.offers.contains_key(&updated_match_id) {
            let offer = state.offers.remove(&updated_match_id).unwrap();
            
            cb(offer.clone(), false);
        }

        let IsFinished = state.matches[&updated_match_id].IsFinished.unwrap_or(false);
        
        if IsFinished {
            debug!("Match is finished: {:?}", state.matches[&updated_match_id]);

            state.matches.remove(&updated_match_id);
        } else {
            let ref match_ = state.matches[&updated_match_id];

            if let Some(offer) = try!(convert_match_into_offer(match_)) {
                state.offers.insert(offer.inner_id as u32, offer.clone());
                
                cb(offer, true);
            } else {
                debug!("Match is not transformed into offer: {:?}", match_);
            }
        }
    }

    Ok(())
}
