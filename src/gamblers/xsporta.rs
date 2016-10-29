#![allow(non_snake_case)]

use std::collections::HashSet;
use serde_json as json;

use base::error::{Result, Error};
use base::timers::Periodic;
use base::parsing::{NodeRefExt, ElementDataExt};
use base::session::Session;
use base::currency::Currency;
use gamblers::{Gambler, Message};
use gamblers::Message::*;
use markets::{OID, Offer, Outcome, DRAW, Game, Kind};

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
        let html = try!(self.session.get_html("/"));

        let raw_auth_dv_elem = try!(html.query("#authDV"));
        let raw_auth_dv = try!(raw_auth_dv_elem.get_attr("value"));

        let mut auth_dv = String::new();

        for code in raw_auth_dv.split('.') {
            let code = try!(code.parse::<u8>());
            auth_dv.push(code as char);
        }

        self.session
            .post_form("/user/auth/", &[
                ("authDV", &auth_dv),
                ("uLogin", username),
                ("uPassword", password)
            ], &[])
            .map(|_| ())
    }

    fn check_balance(&self) -> Result<Currency> {
        let text = try!(self.session.get_text("/en/user/checkUserBalance.php"));
        let on_invalid_balance = || format!("Invalid balance: {}", text);
        let balance_str = try!(text.split(' ').next().ok_or_else(on_invalid_balance));
        let balance = try!(balance_str.parse::<f64>());

        Ok(Currency::from(balance))
    }

    fn watch(&self, cb: &Fn(Message)) -> Result<()> {
        let path = "/LineFeed/Get1x2?sportId=40&count=50&cnt=10&lng=en";
        let mut active = HashSet::new();

        // The site uses 1-minute period, but for us it's too long.
        for _ in Periodic::new(15) {
            let message = try!(self.session.get_json::<XMessage>(path));
            let offers = try!(grab_offers(message));

            // Deactive active offers.
            for offer in &offers {
                active.remove(&offer.oid);
            }

            // Now `active` contains inactive.
            for oid in active.drain() {
                cb(Remove(oid))
            }

            // Add/update offers.
            for offer in offers {
                active.insert(offer.oid);
                cb(Upsert(offer));
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
        let request = PlaceBetRequest {
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

        let raw_response = try!(self.session.post_json(path, request));
        let response: PlaceBetResponse = try!(json::from_reader(raw_response));

        if !response.Success {
            return Err(From::from(response.Error));
        }

        Ok(())
    }

    fn check_offer(&self, offer: &Offer, outcome: &Outcome, stake: Currency) -> Result<bool> {
        let path = "/LineFeed/Get1x2?sportId=40&count=50&cnt=10&lng=en";
        let message = try!(self.session.get_json::<XMessage>(path));

        Ok(try!(grab_offers(message)).into_iter()
            .find(|actual| actual.oid == offer.oid)
            // TODO(loyd): change it after #78.
            .map_or(true, |actual| &actual == offer && actual.outcomes == offer.outcomes))
    }
}

#[derive(Deserialize)]
struct XMessage {
    Error: String,
    Success: bool,
    Value: Vec<Info>
}

#[derive(Deserialize)]
struct Info {
    // TODO(loyd): what is the difference between `ConstId`, `Id` and `MainGameId`?
    Id: u32,
    ChampEng: String,
    Opp1: String,
    Opp2: String,
    Start: u32,
    Events: Vec<Event>
}

#[derive(Deserialize)]
struct Event {
    C: f64,
    T: u32
}

#[derive(Serialize, Debug)]
struct PlaceBetRequest {
    Events: Vec<PlaceBetRequestEvent>,
    Summ: String,
    UserId: String,
    hash: String
}

#[derive(Serialize, Debug)]
struct PlaceBetRequestEvent {
    GameId: u32,
    Coef: f64,
    Kind: u32,
    Type: u32
}

#[derive(Deserialize, Debug)]
struct PlaceBetResponse {
    Error: String,
    Success: bool
}

fn grab_offers(message: XMessage) -> Result<Vec<Offer>> {
    if !message.Success {
        return Err(From::from(message.Error));
    }

    let offers = message.Value.into_iter().filter_map(|info| {
        let coef_1 = info.Events.iter().find(|ev| ev.T == 1).map(|ev| ev.C);
        let coef_2 = info.Events.iter().find(|ev| ev.T == 3).map(|ev| ev.C);

        if coef_1.is_none() || coef_2.is_none() {
            return None;
        }

        let champ = &info.ChampEng;

        let game = match &champ[..4] {
            "CS:G" | "Coun" => Game::CounterStrike,
            "Dota" => Game::Dota2,
            "Hero" => Game::HeroesOfTheStorm,
            "Hear" => Game::Hearthstone,
            "Leag" | "LoL " => Game::LeagueOfLegends,
            "Over" => Game::Overwatch,
            "Smit" => Game::Smite,
            "Star" => Game::StarCraft2,
            "Worl" => Game::WorldOfTanks,
            _ if champ.contains("StarCraft") => Game::StarCraft2,
            "WarC" => return None,
            _ => {
                warn!("Unknown game: {}", info.ChampEng);
                return None;
            }
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
    }).collect();

    Ok(offers)
}
