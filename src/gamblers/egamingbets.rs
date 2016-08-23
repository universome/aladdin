use chrono::NaiveDateTime;

use base::Prime;
use base::{NodeRefExt, ElementDataExt};
use base::{Session, Currency};
use gamblers::Gambler;
use events::{Event, Game, Kind};

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
    fn authorize(&self, username: &str, password: &str) -> Prime<()> {
        let html = try!(self.session.get_html("/"));

        let csrf_elem = try!(html.query(r#"meta[name="csrf-token"]"#));
        let csrf = try!(csrf_elem.get_attr("content"));

        self.session.post_form("/users/sign_in", &[
            ("utf8", "✓"),
            ("authenticity_token", &csrf),
            ("user[name]", username),
            ("user[password]", password),
            ("user[remember_me]", "1")
        ]).map(|_| ())
    }

    fn check_balance(&self) -> Prime<Currency> {
        let balance = try!(self.session.get_json::<Balance>("/user/info?m=1&b=1"));
        let money = try!(balance.bets.parse::<f32>());
        Ok(Currency::from(money))
    }

    fn get_events(&self) -> Prime<Vec<Event>> {
        let table = try!(self.session.get_json::<Table>("/bets?st=0&ut=0&f="));
        let events = table.bets.into_iter().filter_map(Into::into).collect();
        Ok(events)
    }

    fn make_bet(&self, event: Event, outcome: u32, size: Currency) -> Prime<()> {
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

impl Into<Option<Event>> for Bet {
    fn into(self) -> Option<Event> {
        // Irrelevant by date.
        if self.winner > 0 {
            return None;
        }

        let game = match self.game.as_ref() {
            "Dota2" => Game::Dota2,
            _ => return None
        };

        let coef_1 = self.coef_1.parse();
        let coef_2 = self.coef_2.parse();
        let coef_draw = if self.coef_draw == "" { Ok(0.) } else { self.coef_draw.parse() };

        // TODO(loyd): improve error handling.
        if coef_1.is_err() || coef_2.is_err() || coef_draw.is_err() {
            return None;
        }

        let coef_draw_opt = if let Ok(0.) = coef_draw { None } else { Some(coef_draw.unwrap()) };

        Some(Event {
            date: NaiveDateTime::from_timestamp(self.date as i64, 0),
            game: game,
            kind: Kind::OneVsOne {
                team_one: (self.gamer_1.nick, coef_1.unwrap()),
                team_two: (self.gamer_2.nick, coef_2.unwrap()),
                draw: coef_draw_opt
            },
            gamid: self.id as u64
        })
    }
}

#[derive(RustcDecodable)]
struct Gamer {
    nick: String
}
