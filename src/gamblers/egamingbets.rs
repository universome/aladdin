use std::cmp;
use std::collections::{BinaryHeap, HashMap};
use time;

use base::error::Result;
use base::timers::Periodic;
use base::parsing::{NodeRefExt, ElementDataExt};
use base::session::Session;
use base::currency::Currency;
use gamblers::Gambler;
use events::{Offer, Outcome, DRAW, Kind};
use events::{CounterStrike, Dota2, LeagueOfLegends, Overwatch, StarCraft2, WorldOfTanks};

pub struct EGB {
    session: Session
}

impl EGB {
    pub fn new() -> EGB {
        EGB {
            session: Session::new("https://egamingbets.com")
        }
    }
}

impl Gambler for EGB {
    fn authorize(&self, username: &str, password: &str) -> Result<()> {
        let html = try!(self.session.get_html("/"));

        let csrf_elem = try!(html.query(r#"meta[name="csrf-token"]"#));
        let csrf = try!(csrf_elem.get_attr("content"));

        self.session
            .post_form("/users/sign_in", &[
                ("utf8", "✓"),
                ("authenticity_token", &csrf),
                ("user[name]", username),
                ("user[password]", password),
                ("user[remember_me]", "1")
            ])
            .map(|_| ())
    }

    fn check_balance(&self) -> Result<Currency> {
        let balance = try!(self.session.get_json::<Balance>("/user/info?m=1&b=1"));
        let money = try!(balance.bets.parse::<f64>());

        Ok(Currency::from(money))
    }

    fn watch(&self, cb: &Fn(Offer, bool)) -> Result<()> {
        let mut map = HashMap::new();
        let mut heap = BinaryHeap::new();

        let table = try!(self.session.get_json::<Table>("/bets?st=0&ut=0&f="));
        let mut user_time = table.user_time;
        let mut update_time = 0;

        if let Some(bets) = table.bets {
            for bet in bets {
                let id = bet.id;
                update_time = cmp::max(update_time, bet.ut);

                if let Some(offer) = try!(bet.into()) {
                    map.insert(id, offer.clone());
                    heap.push(TimeMarker(-(offer.date as i32), id));
                    cb(offer, true);
                }
            }
        }

        let period = 5;

        for _ in Periodic::new(period) {
            let path = format!("/bets?st={}&ut={}&fg=0&f=", user_time, update_time);
            let table = try!(self.session.get_json::<Table>(&path));
            user_time = table.user_time;

            // Add/update offers.
            if let Some(bets) = table.bets {
                for bet in bets {
                    let id = bet.id;
                    update_time = cmp::max(update_time, bet.ut);

                    if let Some(offer) = try!(bet.into()) {
                        // We assume that offers for the id are equal and store only first.
                        debug_assert!(map.get(&id).map_or(true, |o| &offer == o));

                        if !map.contains_key(&id) {
                            map.insert(id, offer.clone());
                            heap.push(TimeMarker(-(offer.date as i32), id));
                        }

                        cb(offer, true);
                    }
                }
            }

            // Remove old offers.
            let threshold = time::get_time().sec as u32 + period as u32;

            while !heap.is_empty() {
                let &TimeMarker(date, id) = heap.peek().unwrap();

                if -date as u32 > threshold {
                    break;
                }

                heap.pop();
                let offer = map.remove(&id).unwrap();
                cb(offer, false);
            }
        }

        Ok(())
    }

    fn place_bet(&self, offer: Offer, outcome: Outcome, bet: Currency) -> Result<()> {
        unimplemented!();
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct TimeMarker(i32, u32);

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
    id: u32,
    winner: i32,
    live: u8,
    ut: u32
}

#[derive(Deserialize)]
struct Gamer {
    nick: String
}

impl Into<Result<Option<Offer>>> for Bet {
    fn into(self) -> Result<Option<Offer>> {
        let irrelevant = self.winner > 0                            // Ended or cancelled.
                      || self.live == 1                             // Exactly live.
                      || time::get_time().sec as u32 >= self.date   // Started.
                      || self.gamer_1.nick.contains("(Live)")       // Live.
                      || self.gamer_2.nick.contains("(Live)");

        if irrelevant {
            return Ok(None);
        }

        let kind = match self.game.as_ref() {
            "Counter-Strike" => Kind::CounterStrike(CounterStrike::Series),
            "Dota2" => Kind::Dota2(Dota2::Series),
            "LoL" => Kind::LeagueOfLegends(LeagueOfLegends::Series),
            "Overwatch" => Kind::Overwatch(Overwatch::Series),
            "StarCraft2" => Kind::StarCraft2(StarCraft2::Series),
            "WorldOfTanks" => Kind::WorldOfTanks(WorldOfTanks::Series),
            _ => return Ok(None)
        };

        let coef_1 = try!(self.coef_1.parse());
        let coef_2 = try!(self.coef_2.parse());
        let coef_draw = if self.coef_draw == "" { 0. } else { try!(self.coef_draw.parse()) };

        let mut outcomes = vec![
            Outcome(self.gamer_1.nick, coef_1),
            Outcome(self.gamer_2.nick, coef_2)
        ];

        if coef_draw > 0. {
            outcomes.push(Outcome(DRAW.to_owned(), coef_draw));
        }

        Ok(Some(Offer {
            date: self.date,
            kind: kind,
            outcomes: outcomes,
            inner_id: self.id as u64
        }))
    }
}
