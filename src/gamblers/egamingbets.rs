use std::time::Duration;
use std::thread;
use chrono::{NaiveDateTime, DateTime, UTC};

use base::error::Result;
use base::timers::Periodic;
use base::parsing::{NodeRefExt, ElementDataExt};
use base::session::Session;
use base::currency::Currency;
use gamblers::Gambler;
use events::{Offer, Outcome, DRAW, Kind, Dota2};

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

    fn watch(&self, cb: &Fn(Offer)) -> Result<()> {
        let table = try!(self.session.get_json::<Table>("/bets?st=0&ut=0&f="));

        for bet in table.bets {
            if let Some(offer) = try!(bet.into()) {
                cb(offer);
            }
        }

        Ok(())
    }

    fn place_bet(&self, offer: Offer, outcome: Outcome, bet: Currency) -> Result<()> {
        unimplemented!();
    }
}

#[derive(RustcDecodable)]
struct Balance {
    bets: String
}

#[derive(RustcDecodable)]
struct Table {
    // TODO(loyd): also check `nested_bets`.
    bets: Vec<Bet>
}

#[derive(RustcDecodable)]
struct Bet {
    game: String,
    date: u32,
    coef_1: String,
    coef_2: String,
    coef_draw: String,
    gamer_1: Gamer,
    gamer_2: Gamer,
    id: u32,
    winner: i32
}

#[derive(RustcDecodable)]
struct Gamer {
    nick: String
}

impl Into<Result<Option<Offer>>> for Bet {
    fn into(self) -> Result<Option<Offer>> {
        // Irrelevant by date.
        if self.winner > 0 {
            return Ok(None);
        }

        let kind = match self.game.as_ref() {
            "Dota2" => Kind::Dota2(Dota2::Series),
            _ => return Ok(None)
        };

        let coef_1 = try!(self.coef_1.parse());
        let coef_2 = try!(self.coef_2.parse());
        let coef_draw = if self.coef_draw == "" { 0. } else { try!(self.coef_draw.parse()) };

        let nick_1 = self.gamer_1.nick.replace(" (Live)", "");
        let nick_2 = self.gamer_2.nick.replace(" (Live)", "");

        let mut outcomes = vec![
            Outcome(nick_1, coef_1),
            Outcome(nick_2, coef_2)
        ];

        if coef_draw > 0. {
            outcomes.push(Outcome(DRAW.to_owned(), coef_draw));
        }

        Ok(Some(Offer {
            date: DateTime::from_utc(NaiveDateTime::from_timestamp(self.date as i64, 0), UTC),
            kind: kind,
            outcomes: outcomes,
            inner_id: self.id as u64
        }))
    }
}
