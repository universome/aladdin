use std::cmp;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::Relaxed;
use std::collections::{BinaryHeap, HashMap};
use kuchiki::NodeRef;
use time;

use base::error::{Result, Error};
use base::timers::Periodic;
use base::parsing::{NodeRefExt, ElementDataExt};
use base::session::{Session, Type};
use base::currency::Currency;
use gamblers::{Gambler, Message};
use gamblers::Message::*;
use markets::{OID, Offer, Outcome, DRAW, Game, Kind};

pub struct EGB {
    session: Session,
    csrf: Mutex<String>,
    user_time: AtomicUsize,
    update_time: AtomicUsize
}

impl EGB {
    pub fn new() -> EGB {
        EGB {
            session: Session::new("egamingbets.com"),
            csrf: Mutex::new(String::new()),
            user_time: AtomicUsize::new(0),
            update_time: AtomicUsize::new(0)
        }
    }
}

impl Gambler for EGB {
    fn authorize(&self, username: &str, password: &str) -> Result<()> {
        let html: NodeRef = try!(self.session.request("/").get());
        let csrf = try!(extract_csrf(html));

        try!(self.session.request("/egb_users/sign_in")
            .headers(&[("X-CSRF-Token", &csrf)])
            .content_type(Type::Form)
            .post::<String, _>(vec![
                ("utf8", "✓"),
                ("authenticity_token", &csrf),
                ("egb_user[name]", username),
                ("egb_user[password]", password),
                ("egb_user[remember_me]", "1")
            ]));

        let html: NodeRef = try!(self.session.request("/tables").get());
        let csrf = try!(extract_csrf(html));

        let mut guard = self.csrf.lock().unwrap();
        *guard = csrf;

        Ok(())
    }

    fn check_balance(&self) -> Result<Currency> {
        let balance = try!(self.session.request("/user/info?m=1&b=1").get::<Balance>());
        let money = try!(balance.bets.parse::<f64>());

        Ok(Currency::from(money))
    }

    fn watch(&self, cb: &Fn(Message)) -> Result<()> {
        #[derive(PartialEq, Eq, PartialOrd, Ord)]
        struct TimeMarker(i32, OID);

        let mut map = HashMap::new();
        let mut heap = BinaryHeap::new();

        let table: Table = try!(self.session.request("/bets?st=0&ut=0&f=").get());
        let mut user_time = table.user_time;
        let mut update_time = 0;

        if let Some(bets) = table.bets {
            for bet in bets {
                let id = bet.id;
                update_time = cmp::max(update_time, bet.ut);

                if let Some(offer) = try!(extract_offer(bet)) {
                    map.insert(id, offer.clone());
                    heap.push(TimeMarker(-(offer.date as i32), id));
                    cb(Upsert(offer))
                }
            }
        }

        self.user_time.store(user_time as usize, Relaxed);
        self.update_time.store(update_time as usize, Relaxed);

        let period = 5;

        for _ in Periodic::new(period) {
            let path = format!("/bets?st={}&ut={}&fg=0&f=", user_time, update_time);
            let table: Table = try!(self.session.request(path.as_ref()).get());
            user_time = table.user_time;

            // Add/update offers.
            if let Some(bets) = table.bets {
                for bet in bets {
                    let id = bet.id;
                    update_time = cmp::max(update_time, bet.ut);

                    let offer = match try!(extract_offer(bet)) {
                        Some(offer) => offer,
                        None => continue
                    };

                    // Short case: a new offer.
                    if !map.contains_key(&id) {
                        map.insert(id, offer.clone());
                        heap.push(TimeMarker(-(offer.date as i32), id));
                        cb(Upsert(offer));
                        continue;
                    }

                    let stored = map.remove(&id).unwrap();

                    if stored.date != offer.date {
                        heap.push(TimeMarker(-(offer.date as i32), id));
                    }

                    cb(Upsert(offer.clone()));
                    map.insert(id, offer);
                }
            }

            self.user_time.store(user_time as usize, Relaxed);
            self.update_time.store(update_time as usize, Relaxed);

            // Remove old offers.
            let threshold = time::get_time().sec as u32 + period as u32;

            while !heap.is_empty() {
                let &TimeMarker(date, id) = heap.peek().unwrap();

                if -date as u32 > threshold {
                    break;
                }

                heap.pop();

                // Remove offer only if the time marker corresponds to the last modification.
                if map.get(&id).map_or(false, |o| o.date == -date as u32) {
                    let offer = map.remove(&id).unwrap();
                    cb(Remove(offer.oid));
                }
            }
        }

        Ok(())
    }

    fn place_bet(&self, offer: Offer, outcome: Outcome, stake: Currency) -> Result<()> {
        let stake: f64 = stake.into();
        let idx = 1 + offer.outcomes.iter().position(|o| o == &outcome).unwrap();

        let csrf = self.csrf.lock().unwrap();

        let request = self.session.request("/bets")
            .headers(&[("X-CSRF-Token", &*csrf)])
            .content_type(Type::Form);

        let response: PlaceBetResponse = try!(request.post(vec![
            ("bet[id]", offer.oid.to_string().as_ref()),
            ("bet[amount]", stake.to_string().as_ref()),
            ("bet[playmoney]", "false"),
            ("bet[coef]", outcome.1.to_string().as_ref()),
            ("bet[on]", idx.to_string().as_ref()),
            ("bet[type]", "main")
        ]));

        if response.success {
            Ok(())
        } else {
            Err(Error::from(response.message))
        }
    }

    fn check_offer(&self, offer: &Offer, _: &Outcome, _: Currency) -> Result<bool> {
        let user_time = self.user_time.load(Relaxed);
        let update_time = self.update_time.load(Relaxed);

        let path = format!("/bets?st={}&ut={}&fg=0&f=", user_time, update_time);
        let table: Table = try!(self.session.request(path.as_ref()).get());

        if table.bets.is_none() {
            return Ok(true);
        }

        for bet in table.bets.unwrap() {
            if bet.id != offer.oid {
                continue;
            }

            let actual = match try!(extract_offer(bet)) {
                Some(offer) => offer,
                None => return Ok(false)
            };

            return Ok(&actual == offer && actual.outcomes == offer.outcomes);
        }

        return Ok(true);
    }
}

fn extract_csrf(html: NodeRef) -> Result<String> {
    let csrf_elem = try!(html.query(r#"meta[name="csrf-token"]"#));
    csrf_elem.get_attr("content")
}

#[derive(Deserialize)]
struct PlaceBetResponse {
    message: String,
    success: bool
}

#[derive(Deserialize)]
struct Balance {
    bets: String
}

#[derive(Deserialize)]
struct Table {
    user_time: u32,
    bets: Option<Vec<Bet>>
}

#[derive(Deserialize)]
struct Bet {
    game: String,
    date: u32,
    coef_1: String,
    coef_2: String,
    coef_draw: String,
    gamer_1: Gamer,
    gamer_2: Gamer,
    id: u64,
    winner: i32,
    live: u8,
    ut: u32
}

#[derive(Deserialize)]
struct Gamer {
    nick: String
}

fn extract_offer(bet: Bet) -> Result<Option<Offer>> {
    let irrelevant = bet.winner > 0                            // Ended or cancelled.
                  || bet.live == 1                             // Exactly live.
                  || time::get_time().sec as u32 >= bet.date   // Started.
                  || bet.gamer_1.nick.contains("(Live)")       // Live.
                  || bet.gamer_2.nick.contains("(Live)");

    if irrelevant {
        return Ok(None);
    }

    let game = match bet.game.as_ref() {
        "Counter-Strike" => Game::CounterStrike,
        "Dota2" => Game::Dota2,
        "Hearthstone" => Game::Hearthstone,
        "HeroesOfTheStorm" => Game::HeroesOfTheStorm,
        "LoL" => Game::LeagueOfLegends,
        "Overwatch" => Game::Overwatch,
        "Smite" => Game::Smite,
        "StarCraft2" => Game::StarCraft2,
        "WorldOfTanks" => Game::WorldOfTanks,
        game => {
            warn!("Unknown game: {}", game);
            return Ok(None);
        }
    };

    let coef_1 = try!(bet.coef_1.parse());
    let coef_2 = try!(bet.coef_2.parse());
    let coef_draw = if bet.coef_draw == "" { 0. } else { try!(bet.coef_draw.parse()) };

    let mut outcomes = vec![
        Outcome(bet.gamer_1.nick, coef_1),
        Outcome(bet.gamer_2.nick, coef_2)
    ];

    if coef_draw > 0. {
        outcomes.push(Outcome(DRAW.to_owned(), coef_draw));
    }

    Ok(Some(Offer {
        oid: bet.id,
        date: bet.date,
        game: game,
        kind: Kind::Series,
        outcomes: outcomes
    }))
}
