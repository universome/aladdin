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
        // TODO(loyd): removing offers.
        // TODO(loyd): optimize this.
        for _ in Periodic::new(40) {
            let table = try!(self.session.get_json::<Table>("/bets?st=0&ut=0&f="));

            for bet in table.bets {
                if let Some(offer) = try!(bet.into()) {
                    cb(offer, true);
                }
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
        if self.winner > 0 || time::get_time().sec as u32 >= self.date {
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
            date: self.date,
            kind: kind,
            outcomes: outcomes,
            inner_id: self.id as u64
        }))
    }
}
