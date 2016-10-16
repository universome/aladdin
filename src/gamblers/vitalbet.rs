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
use gamblers::Gambler;
use events::{OID, Offer, Outcome, Game, Kind, DRAW};

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
            session: Session::new("vitalbet.com"),
            state: Mutex::new(State {
                odds_to_matches_ids: HashMap::new(),
                matches: HashMap::new(),
                offers: HashMap::new(),
                changed_matches: HashSet::new()
            })
        }
    }

    // TODO(universome): Pass timestamps, like they do.
    fn generate_polling_path(&self) -> Result<String> {
        // First, we should get connection token.
        let auth_path = concat!("/signalr/negotiate?transport=longPolling&clientProtocol=1.5",
                                "&connectionData=%5B%7B%22name%22%3A%22sporttypehub%22%7D%5D");
        let response = try!(self.session.get_json::<PollingAuthResponse>(auth_path));
        let token = response.ConnectionToken;
        let token = utf8_percent_encode(&token, VITALBET_ENCODE_SET).collect::<String>();

        // We should notify them, that we are starting polling (because they do it too).
        try!(self.session.get_raw_json(&format!(concat!("/signalr/start?transport=longPolling",
                                 "&connectionData=%5B%7B%22name%22%3A%22sporttypehub%22%7D%5D",
                                 "clientProtocol=1.5&connectionToken={}"), token)));

        Ok(format!(concat!("/signalr/poll?transport=longPolling&clientProtocol=1.5",
                        "&connectionData=%5B%7B%22name%22%3A%22sporttypehub%22%7D%5D",
                        "&connectionToken={}"), token))
    }

    fn get_all_matches(&self) -> Result<Vec<Match>> {
        let path = "/api/sportmatch/Get?sportID=2357";

        self.session.get_json::<Vec<Match>>(path)
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
        // First of all, we should get initial page to get session cookie.
        try!(self.session.get_html("/"));

        let mut timer = Periodic::new(3600);

        // Fill with initial matches
        let initial_matches = try!(self.get_all_matches());

        for match_ in initial_matches {
            let mut state = try!(self.state.lock());

            try!(apply_match_update(&mut *state, match_));
        }

        {
            let mut state = try!(self.state.lock());

            try!(provide_offers(&mut *state, cb));
        }

        // Start polling
        let polling_path = try!(self.generate_polling_path());

        loop {
            let mut state = try!(self.state.lock());

            // Every hour we should renew our state
            if timer.next_if_elapsed() {
                let matches = try!(self.get_all_matches());

                state.odds_to_matches_ids = HashMap::new();

                for match_ in matches {
                    try!(apply_match_update(&mut *state, match_));
                }

                try!(provide_offers(&mut *state, cb));
            }

            let updates: PollingResponse = try!(self.session.get_json(&polling_path));

            try!(apply_updates(&mut *state, updates.M));
            try!(provide_offers(&mut *state, cb));
        }
    }

    fn place_bet(&self, offer: Offer, outcome: Outcome, stake: Currency) -> Result<()> {
        let state = &*try!(self.state.lock());
        let match_ = match state.matches.get(&(offer.oid as u32)) {
            Some(m) => m,
            None => return Err(Error::from("Match with provided oid is not found"))
        };
        let outcome_id = match match_.PreviewOdds {
            Some(ref odds) => match odds.iter().find(|o| o.Title == outcome.0) {
                Some(ref odd) => odd.ID,
                None => return Err(Error::from("Match does not have odd with odd.Title == outcome.0"))
            },
            None => return Err(Error::from("Match does not have any odds"))
        };

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

        let response = try!(self.session.post_json("/api/betslip/place", request_data));
        let response: PlaceBetResponse = try!(json::from_reader(response));

        match response.ErrorMessage {
            Some(m) => Err(Error::from(m)),
            None => Ok(())
        }
    }
}

#[derive(Debug)]
struct State {
    odds_to_matches_ids: HashMap<u32, u32>,
    matches: HashMap<u32, Match>,
    offers: HashMap<u32, Offer>,
    changed_matches: HashSet<u32>
}

impl State {
    fn new(&self) -> State {
        State {
            matches: HashMap::new(),
            odds_to_matches_ids: HashMap::new(),
            offers: HashMap::new(),
            changed_matches: HashSet::new()
        }
    }
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

    PreviewOdds: Option<Vec<Odd>>,
    IsActive: Option<bool>,
    IsFinished: Option<bool>,
    Category: Option<Category>,
    PreviewMarket: Option<Market>
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

#[derive(Deserialize, Debug)]
struct Market {
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
    fn deserialize<D>(d: &mut D) -> StdResult<PM, D::Error> where D: Deserializer {
        let result: json::Value = try!(Deserialize::deserialize(d));

        if result.find("M").map_or(false, json::Value::is_string) {
            return Ok(PM::UnsupportedUpdate(UnsupportedUpdate("Even no M".to_string())));
        }

        let update_type = result.find("M").unwrap().as_str().unwrap_or("No update type").to_string();

        Ok(match update_type.as_ref() {
            "oddsUpdated" => PM::OddsUpdateMessage( json::from_value(result).unwrap() ),
            "prematchOddsUpdated" => PM::PrematchOddsUpdateMessage( json::from_value(result).unwrap() ),
            "matchesUpdated" => PM::MatchesUpdateMessage( json::from_value(result).unwrap() ),
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
        IsSuspended: update.2 == 3 // IsSuspended status.
    }
}

fn convert_prematch_match_update(update: PrematchMatchUpdate) -> Match {
    let tm = time::at_utc(time::Timespec::new(update.2 as i64, 0));

    Match {
        ID: update.0,
        IsSuspended: update.1 == 3, // IsSuspended status.
        DateOfMatch: time::strftime("%Y-%m-%dT%H:%M:%S", &tm).unwrap(),

        IsFinished: None,
        PreviewOdds: None,
        IsActive: None,
        Category: None,
        PreviewMarket: None
    }
}

fn convert_match_into_offer(match_: &Match) -> Result<Option<Offer>> {
    let game_and_kind = get_game_and_kind(&match_);

    if match_.IsSuspended || !match_.IsActive.unwrap_or(true) || game_and_kind.is_none() {
        return Ok(None);
    }

    match match_.PreviewMarket {
        Some(Market { ref Name }) => {
            // Currently, we are interested only in a special market types.
            if Name != "Match Odds" && Name != "Match Odds (3 Way)" && Name != "Series Winner" {
                return Ok(None);
            }
        },
        None => {
            warn!("We have a match without PreviewMarket: {:?}", match_);
            return Ok(None);
        }
    }

    match match_.PreviewOdds {
        Some(ref odds) => {
            if odds.len() < 2 || odds.iter().any(|o| o.IsSuspended) {
                return Ok(None);
            }
        },
        None => return Ok(None)
    }

    let odds = match match_.PreviewOdds {
        Some(ref odds) => odds.iter()
            .map(|odd| {
                let title = if odd.Title == "Draw" { DRAW.to_owned() } else { odd.Title.clone() };

                Outcome(title, odd.Value)
            })
            .collect::<Vec<_>>(),
        None => return Ok(None)
    };

    let ts = try!(time::strptime(&match_.DateOfMatch, "%Y-%m-%dT%H:%M:%S")).to_timespec();

    let (game, kind) = game_and_kind.unwrap();

    Ok(Some(Offer {
        oid: match_.ID as OID,
        date: ts.sec as u32,
        game: game,
        kind: kind,
        outcomes: odds
    }))
}

fn get_game_and_kind(match_: &Match) -> Option<(Game, Kind)> {
    if match_.Category.is_none() {
        return None;
    }

    Some((match match_.Category.as_ref().unwrap().ID {
        3578 => Game::LeagueOfLegends,
        3597 => Game::HeroesOfTheStorm,
        3598 => Game::Hearthstone,
        3600 => Game::Smite,
        3601 => Game::WorldOfTanks,
        3683 => Game::CounterStrike,
        3693 => Game::Dota2,
        3704 => Game::StarCraft2,
        5791 => Game::Overwatch,
        5942 => Game::Halo,
        6241 => Game::CrossFire,
        _ => {
            warn!("New category in vitalbet esports: {:?}", match_.Category);
            return None
        }
    }, Kind::Series))
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
            PM::MatchesUpdateMessage(msg) => {
                for match_update in msg.A.into_iter().flat_map(|updates| updates.into_iter()) {
                    try!(apply_match_update(state, match_update));
                }
            },
            PM::PrematchMatchesUpdateMessage(msg) => {
                for match_update in msg.A.into_iter().flat_map(|updates| updates.into_iter()) {
                    try!(apply_match_update(state, convert_prematch_match_update(match_update)));
                }
            },
            _ => {}
        }
    }

    Ok(())
}

fn apply_odd_update(state: &mut State, odd_update: &OddUpdate) -> Result<()> {
    if !state.odds_to_matches_ids.contains_key(&odd_update.ID) {
        // This is an update for some odd, which we do not track.
        return Ok(());
    }

    let match_id = state.odds_to_matches_ids[&odd_update.ID];

    if !state.matches.contains_key(&match_id) {
        // This is an update for some match, which we do not track.
        return Ok(());
    }

    let match_ = state.matches.get_mut(&match_id).unwrap();

    // Find the odd we want to update and update it.
    if let Some(ref mut odds) = match_.PreviewOdds {
        for odd in odds {
            if odd.ID == odd_update.ID {
                odd.Value = odd_update.Value;
                odd.IsSuspended = odd_update.IsSuspended;
            }
        }

        state.changed_matches.insert(match_.ID);
    }

    Ok(())
}

fn apply_match_update(state: &mut State, match_update: Match) -> Result<()> {
    state.changed_matches.insert(match_update.ID);

    if state.matches.contains_key(&match_update.ID) {
        let match_ = state.matches.get_mut(&match_update.ID).unwrap();

        match_.IsSuspended = match_update.IsSuspended;
        match_.DateOfMatch = match_update.DateOfMatch;

        if let Some(odds) = match_update.PreviewOdds {
            for odd in &odds {
                state.odds_to_matches_ids.insert(odd.ID, match_.ID);
            }

            match_.PreviewOdds = Some(odds);
        }
    } else {
        state.matches.insert(match_update.ID, match_update);
    }

    Ok(())
}

fn provide_offers(state: &mut State, cb: &Fn(Offer, bool)) -> Result<()> {
    for updated_match_id in state.changed_matches.drain() {
        if let Some(offer) = try!(convert_match_into_offer(&state.matches[&updated_match_id])) {
            state.offers.insert(offer.oid as u32, offer.clone());

            cb(offer, true);
        } else {
            if let Some(offer) = state.offers.remove(&updated_match_id) {
                cb(offer, false);
            }

            if state.matches[&updated_match_id].IsFinished.unwrap_or(false) {
                debug!("Match is finished: {:?}", state.matches[&updated_match_id]);

                state.matches.remove(&updated_match_id);
            }
        }
    }

    Ok(())
}
