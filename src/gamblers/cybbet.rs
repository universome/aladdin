#![allow(non_snake_case)]

use std::collections::HashMap;
use kuchiki::{self, NodeRef};
use kuchiki::traits::TendrilSink;
use serde_json as json;
use time;

use base::error::{Result, Error};
use base::timers::Periodic;
use base::parsing::{NodeRefExt, ElementDataExt};
use base::session::{Session, Type};
use base::currency::Currency;
use gamblers::{Gambler, Message};
use gamblers::Message::*;
use markets::{Offer, Outcome, DRAW, Game, Kind};

// The site uses 1-minute period, but for us it's too long.
const PERIOD: u32 = 30;

pub struct CybBet {
    session: Session
}

impl CybBet {
    pub fn new() -> CybBet {
        CybBet {
            session: Session::new("cybbet.com")
        }
    }

    fn try_place_bet(&self, path: &str,
                     offer: &Offer, outcome: &Outcome, stake: Currency) -> Result<String>
    {
        let stake: f64 = stake.into();

        let result = if outcome.0 == DRAW { 0 } else {
            1 + offer.outcomes.iter().position(|o| o == outcome).unwrap()
        };

        let bets = format!(r#"{{
            "single": [{{
                "gameId": "{id}",
                "subGameId": "undefined",
                "result": "{result}",
                "isSubgame": "0",
                "isTournament": "0",
                "type": "single",
                "koef": {coef},
                "tipMoney": "2",
                "summ": {stake}
            }}],
            "express": [],
            "expressGame": []
        }}"#,
            id = offer.oid,
            result = result,
            coef = outcome.1,
            stake = stake);

        let request = self.session.request(path).content_type(Type::Form);
        let response: String = try!(request.post(vec![("bets", &bets)]));

        Ok(response)
    }
}

impl Gambler for CybBet {
    fn authorize(&self, username: &str, password: &str) -> Result<()> {
        self.session.request("/user/login")
            .content_type(Type::Form)
            .follow_redirects(true)
            .post::<String, _>(vec![
                ("LoginForm[username]", username),
                ("LoginForm[password]", password),
                ("signin_submit", "Sign In")
            ]).map(|_| ())
    }

    fn check_balance(&self) -> Result<Currency> {
        let path = "/account/usercash/UpdateBlockMainUserInfo";
        let html: NodeRef = try!(self.session.request(path).get());
        let text = try!(html.query(r#"a[href="/account/usercash/cash"]"#)).text_contents();
        let on_invalid_cash = || format!("Invalid cash: \"{}\"", text);
        let cash_str = try!(text.split(' ').next().ok_or_else(on_invalid_cash));
        let cash = try!(cash_str.parse::<f64>());

        Ok(Currency::from(cash))
    }

    fn watch(&self, cb: &Fn(Message)) -> Result<()> {
        let html = try!(self.session.request("/").get::<NodeRef>());
        let offers = try!(extract_offers(html));

        let mut table = HashMap::new();

        for offer in offers {
            table.insert(offer.oid as u32, offer.clone());
            cb(Upsert(offer));
        }

        for _ in Periodic::new(PERIOD) {
            // Collect all active offers and send them.
            let games = table.values().map(CGame::from).collect::<Vec<_>>();
            let games = try!(json::to_string(&games));
            let request = self.session.request("/games/getCurrentKoef").content_type(Type::Form);
            let koef: CurrentKoef = try!(request.post(&[("request", &games)]));

            // Update odds.
            if let Some(games) = koef.games {
                for (id, coef_1, coef_2, coef_draw) in games {
                    if !table.contains_key(&id) {
                        continue;
                    }

                    let offer = table.get_mut(&id).unwrap();
                    offer.outcomes[0].1 = coef_1;
                    offer.outcomes[1].1 = coef_2;

                    if coef_draw > 0. {
                        offer.outcomes[2].1 = coef_draw;
                    } else {
                        debug_assert_eq!(offer.outcomes.len(), 2);
                    }

                    cb(Upsert(offer.clone()));
                }
            }

            // Remove started games.
            if let Some(games) = koef.gamesStarted {
                for (id, _) in games {
                    let id = try!(id.parse());

                    if let Some(offer) = table.remove(&id) {
                        cb(Remove(offer.oid));
                    }
                }
            }

            // Request additional info about new games.
            if let Some(games) = koef.gamesStartTime {
                let relevant = try!(filter_relevant(games));

                for (id, date) in relevant {
                    if table.contains_key(&id) {
                        if table[&id].date != date {
                            table.get_mut(&id).map(|o| o.date = date);
                            cb(Upsert(table[&id].clone()))
                        }

                        continue;
                    }

                    let response: String = try!(self.session.request("/games/addNewGame")
                        .content_type(Type::Form)
                        .post(&[("idGame", &id.to_string())]));

                    // Fix invalid markup to parse this bullshit below.
                    let template = format!("<table>{}</table>", response);
                    let html = kuchiki::parse_html().one(template);

                    let mut offers = try!(extract_offers(html));

                    if !offers.is_empty() {
                        let offer = offers.drain(..).next().unwrap();
                        cb(Upsert(offer.clone()));
                        table.insert(id, offer);
                    }
                }
            }
        }

        Ok(())
    }

    fn place_bet(&self, offer: Offer, outcome: Outcome, stake: Currency) -> Result<()> {
        let response = try!(self.try_place_bet("/games/bet", &offer, &outcome, stake));

        if response.contains("messageSuccess") {
            Ok(())
        } else {
            // TODO(loyd): what about doing something more clever?
            Err(Error::from(response))
        }
    }

    fn check_offer(&self, offer: &Offer, outcome: &Outcome, stake: Currency) -> Result<bool> {
        let response = try!(self.try_place_bet("/games/checkbet", &offer, &outcome, stake));
        Ok(response.contains("warning\":\"\""))
    }
}

type Trash = json::Value;

#[derive(Deserialize)]
struct CurrentKoef {
    games: Option<Vec<(u32, f64, f64, f64)>>,
    gamesStarted: Option<Vec<(String, Trash)>>,
    gamesStartTime: Option<Vec<(String, String, Trash, Trash, Trash)>>
}

#[derive(Serialize)]
struct CGame {
    idGame: u32,
    team1: f64,
    team2: f64,
    draw: Option<f64>,
    gameStart: u32
}

impl<'a> From<&'a Offer> for CGame {
    fn from(offer: &'a Offer) -> CGame {
        CGame {
            idGame: offer.oid as u32,
            team1: offer.outcomes[0].1,
            team2: offer.outcomes[1].1,
            draw: offer.outcomes.get(2).map(|o| o.1),
            gameStart: offer.date
        }
    }
}

fn extract_offers(html: NodeRef) -> Result<Vec<Offer>> {
    let mut offers = Vec::new();

    for tr in try!(html.query_all("tr.noResult[data-game-id]")) {
        let trn = tr.as_node();

        let img_game_sprite_class = try!(try!(trn.query(".img_game_sprite")).get_attr("class"));
        let game_class_iter = img_game_sprite_class.split_whitespace();
        let kind_class = try!(game_class_iter.last().ok_or("Empty \"class\" attribute"));

        if try!(trn.query_all(".disabled")).next().is_some() {
            continue;
        }

        let game = match kind_class {
            "csgo" => Game::CounterStrike,
            "dota2" =>Game::Dota2,
            "hearthstone" => Game::Hearthstone,
            "heroesofthestorm" => Game::HeroesOfTheStorm,
            "lol" => Game::LeagueOfLegends,
            "ovw" => Game::Overwatch,
            "smite" => Game::Smite,
            "starcraft2" => Game::StarCraft2,
            "VG" => Game::Vainglory,
            "wot" => Game::WorldOfTanks,
            "cod" | "warcraft3" | "warcraft" | "wc3" => continue,
            class => {
                warn!("Unknown game: {}", class);
                continue;
            }
        };

        let team_1 = try!(trn.query(".team-name-first .team-name-text")).text_contents();
        let team_2 = try!(trn.query(".team-name-second .team-name-text")).text_contents();

        if team_1.contains("(Live)") || team_2.contains("(Live)") {
            continue;
        }

        let id = try!(tr.get_attr("data-game-id"));
        let date = try!(tr.get_attr("data-game-start"));

        let coef_1 = try!(trn.query(".team-name-first + .price span")).text_contents();
        let coef_2 = try!(trn.query(".team-name-second + .price span")).text_contents();
        let coef_draw = try!(trn.query_all(".draw .price")).next().map(|s| s.text_contents());

        let mut outcomes = vec![
            Outcome(team_1.to_owned(), try!(coef_1.trim().parse())),
            Outcome(team_2.to_owned(), try!(coef_2.trim().parse()))
        ];

        if let Some(coef_draw) = coef_draw {
            outcomes.push(Outcome(DRAW.to_owned(), try!(coef_draw.trim().parse())));
        }

        offers.push(Offer {
            oid: try!(id.parse()),
            date: try!(date.parse()),
            game: game,
            kind: Kind::Series,
            outcomes: outcomes
        })
    }

    Ok(offers)
}

fn filter_relevant(games: Vec<(String, String, Trash, Trash, Trash)>) -> Result<Vec<(u32, u32)>> {
    let mut relevant = Vec::new();
    let threshold = time::get_time().sec as u32 + PERIOD;

    for (id, date, _, _, _) in games {
        let date = try!(date.parse());

        if date < threshold {
            continue;
        }

        let id = try!(id.parse());
        relevant.push((id, date));
    }

    Ok(relevant)
}
