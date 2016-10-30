#![allow(non_snake_case)]

use std::collections::HashSet;
use kuchiki::NodeRef;

use base::error::{Result, Error};
use base::timers::Periodic;
use base::parsing::{NodeRefExt, ElementDataExt};
use base::session::{Session, Type};
use base::currency::Currency;
use gamblers::{Gambler, Message};
use gamblers::Message::*;
use markets::{OID, Offer, Outcome, DRAW, Game, Kind};

static SPORTS_IDS: &[u32] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 14, 15, 16, 17, 18, 19, 21, 22,
                              23, 24, 26, 27, 28, 30, 31, 32, 36, 38, 40, 41, 49, 56, 66, 67, 80];

pub struct XBet {
    session: Session
}

impl XBet {
    pub fn new() -> XBet {
        XBet {
            session: Session::new("1xsporta.space")
        }
    }
}

impl Gambler for XBet {
    fn authorize(&self, username: &str, password: &str) -> Result<()> {
        let html: NodeRef = try!(self.session.request("/").get());

        let raw_auth_dv_elem = try!(html.query("#authDV"));
        let raw_auth_dv = try!(raw_auth_dv_elem.get_attr("value"));

        let mut auth_dv = String::new();

        for code in raw_auth_dv.split('.') {
            let code = try!(code.parse::<u8>());
            auth_dv.push(code as char);
        }

        self.session.request("/user/auth/")
            .content_type(Type::Form)
            .post::<String, _>(vec![
                ("authDV", auth_dv.as_ref()),
                ("uLogin", username),
                ("uPassword", password)
            ])
            .map(|_| ())
    }

    fn check_balance(&self) -> Result<Currency> {
        let text: String = try!(self.session.request("/en/user/checkUserBalance.php").get());
        let on_invalid_balance = || format!("Invalid balance: {}", text);
        let balance_str = try!(text.split(' ').next().ok_or_else(on_invalid_balance));
        let balance = try!(balance_str.parse::<f64>());

        Ok(Currency::from(balance))
    }

    fn watch(&self, cb: &Fn(Message)) -> Result<()> {
        let mut state = SPORTS_IDS.iter()
            .map(|id| (
                format!("/LineFeed/Get1x2?sportId={}&count=50&cnt=10&lng=en", id),
                HashSet::new()
            ))
            .collect::<Vec<_>>();

        // The site uses 1-minute period, but for us it's too long.
        for _ in Periodic::new(24) {
            for &mut (ref path, ref mut active) in &mut state {
                let message = try!(self.session.request(&path).get::<Get1x2Response>());

                if !message.Success {
                    return Err(Error::from(message.Error));
                }

                let offers = message.Value.into_iter().filter_map(grab_offer).collect::<Vec<_>>();

                // Deactivate active offers.
                for offer in &offers {
                    active.remove(&offer.oid);
                }

                // Now `active` contains inactive.
                for oid in active.drain() {
                    cb(Remove(oid));
                }

                // Add/update offers.
                for offer in offers {
                    active.insert(offer.oid);
                    cb(Upsert(offer));
                }
            }
        }

        Ok(())
    }

    fn place_bet(&self, offer: Offer, outcome: Outcome, stake: Currency) -> Result<()> {
        let stake: f64 = stake.into();
        let hash = self.session.get_cookie("uhash").unwrap();
        let user_id = self.session.get_cookie("ua").unwrap();
        let result = match offer.outcomes.iter().position(|o| o == &outcome).unwrap() {
            0 => 1,
            1 => 3,
            2 => 2,
            _ => return Err(Error::from("Outcome not found in offer"))
        };

        let path = "/en/dataLineLive/put_bets_common.php";
        let request_data = PlaceBetRequest {
            Events: vec![
                PlaceBetRequestEvent {
                    GameId: offer.oid as u32,
                    Coef: outcome.1,
                    Kind: 3,
                    Type: result
                }
            ],
            Summ: stake.to_string(),
            UserId: user_id,
            hash: hash
        };

        let response: PlaceBetResponse = try!(self.session.request(&path).post(request_data));

        if !response.Success {
            return Err(From::from(response.Error));
        }

        Ok(())
    }

    fn check_offer(&self, offer: &Offer, _: &Outcome, _: Currency) -> Result<bool> {
        let path = format!("/LineFeed/GetGame?id={}&count=50&cnt=10&lng=en", offer.oid);
        let message = try!(self.session.request(&path).get::<GetGameResponse>());

        if !message.Success || message.Value.is_none() {
            if message.Error.contains("not found") {
                return Ok(false);
            } else {
                return Err(Error::from(message.Error));
            }
        }

        if let Some(recent) = grab_offer(message.Value.unwrap()) {
            // TODO(loyd): change it after #78.
            Ok(&recent == offer && recent.outcomes == offer.outcomes)
        } else {
            Ok(false)
        }
    }
}

#[derive(Deserialize)]
struct Get1x2Response {
    Error: String,
    Success: bool,
    Value: Vec<Info>
}

#[derive(Deserialize)]
struct GetGameResponse {
    Error: String,
    Success: bool,
    Value: Option<Info>
}

#[derive(Deserialize)]
struct Info {
    // TODO(loyd): what is the difference between `ConstId`, `Id` and `MainGameId`?
    Id: u32,
    ChampEng: String,
    SportNameEng: String,
    Opp1: String,
    Opp2: String,
    Start: u32,
    Events: Vec<Event>
}

#[derive(Deserialize)]
struct Event {
    B: bool,    // It looks like a block flag.
    C: f64,
    T: u32
}

#[derive(Serialize)]
struct PlaceBetRequest {
    Events: Vec<PlaceBetRequestEvent>,
    Summ: String,
    UserId: String,
    hash: String
}

#[derive(Serialize)]
struct PlaceBetRequestEvent {
    GameId: u32,
    Coef: f64,
    Kind: u32,
    Type: u32
}

#[derive(Deserialize)]
struct PlaceBetResponse {
    Error: String,
    Success: bool
}

fn grab_offer(info: Info) -> Option<Offer> {
    // I'm not sure, but `.B` looks like a block flag.
    if info.Events.iter().any(|ev| ev.B && 0 < ev.T && ev.T <= 3) {
        trace!("#{} is blocked (?)", info.Id);
        return None;
    }

    let coef_1 = info.Events.iter().find(|ev| ev.T == 1).map(|ev| ev.C);
    let coef_2 = info.Events.iter().find(|ev| ev.T == 3).map(|ev| ev.C);

    if coef_1.is_none() || coef_2.is_none() {
        return None;
    }

    let game = match game_from_info(&info) {
        Some(game) => game,
        None => return None
    };

    let coef_draw = info.Events.iter().find(|ev| ev.T == 2).map(|ev| ev.C);
    let date = info.Start;
    let id = info.Id;

    let mut outcomes = vec![
        Outcome(info.Opp1, coef_1.unwrap()),
        Outcome(info.Opp2, coef_2.unwrap())
    ];

    if let Some(coef) = coef_draw {
        outcomes.push(Outcome(DRAW.to_owned(), coef));
    }

    Some(Offer {
        oid: id as OID,
        date: date,
        game: game,
        kind: Kind::Series,
        outcomes: outcomes
    })
}

fn game_from_info(info: &Info) -> Option<Game> {
    Some(match info.SportNameEng.as_str() {
        "Alpine Skiing" => Game::AlpineSkiing,
        "American Football" => Game::AmericanFootball,
        "Badminton" => Game::Badminton,
        "Bandy" => Game::Bandy,
        "Baseball" => Game::Baseball,
        "Basketball" => Game::Basketball,
        "Biathlon" => Game::Biathlon,
        "Bicycle Racing" => Game::BicycleRacing,
        "Bowls" => Game::Bowls,
        "Boxing" => Game::Boxing,
        "Chess" => Game::Chess,
        "Cricket" => Game::Cricket,
        "Darts" => Game::Darts,
        "Field Hockey" => Game::FieldHockey,
        "Floorball" => Game::Floorball,
        "Football" => Game::Football,
        "Formula 1" => Game::Formula,
        "Futsal" => Game::Futsal,
        "Gaelic Football" => Game::GaelicFootball,
        "Golf" => Game::Golf,
        "Handball" => Game::Handball,
        "Ice Hockey" => Game::IceHockey,
        "Martial Arts" => Game::MartialArts,
        "Motorbikes" => Game::Motorbikes,
        "Motorsport" => Game::Motorsport,
        "Netball" => Game::Netball,
        "Poker" => Game::Poker,
        "Rugby" => Game::Rugby,
        "Ski Jumping" => Game::SkiJumping,
        "Skiing" => Game::Skiing,
        "Snooker" => Game::Snooker,
        "Table Tennis" => Game::TableTennis,
        "Tennis" => Game::Tennis,
        "Volleyball" => Game::Volleyball,

        "eSports" => match &info.ChampEng[..4] {
            "CS:G" | "Coun" => Game::CounterStrike,
            "Dota" => Game::Dota2,
            "Hero" => Game::HeroesOfTheStorm,
            "Hear" => Game::Hearthstone,
            "Leag" | "LoL " => Game::LeagueOfLegends,
            "Over" => Game::Overwatch,
            "Smit" => Game::Smite,
            "Star" => Game::StarCraft2,
            "Worl" => Game::WorldOfTanks,
            _ if info.ChampEng.contains("StarCraft") => Game::StarCraft2,
            "WarC" => return None,
            _ => {
                warn!("Unknown eSport game: \"{}\"", info.ChampEng);
                return None;
            }
        },

        name => {
            warn!("Unknown sport name: \"{}\"", name);
            return None;
        }
    })
}
