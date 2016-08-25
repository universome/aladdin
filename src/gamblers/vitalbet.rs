use chrono::{NaiveDateTime, DateTime, UTC};
use rustc_serialize::json;

use base::Prime;
use base::{Session, Currency};
use gamblers::Gambler;
use events::{Event, Outcome, Kind, Dota2};

pub struct VitalBet {
    session: Session
}

impl VitalBet {
    pub fn new() -> VitalBet {
        VitalBet {
            session: Session::new("https://vitalbet.com")
        }
    }
}

impl Gambler for VitalBet {
    fn authorize(&self, username: &str, password: &str) -> Prime<()> {
        let body = VitalBetAuthData {
            BrowserFingerPrint: 426682306,
            Login: username,
            Password: password,
            RememberMe: true
        };

        self.session.post_json("/api/authorization/post", body).map(|_| ())
    }

    fn check_balance(&self) -> Prime<Currency> {
        let balance = try!(self.session.get_json::<Balance>("/api/account"));
        let money = balance.Balance;
        
        Ok(Currency::from(money))
    }

    fn get_events(&self) -> Prime<Vec<Event>> {
        // TOOD(universome): we should get events from other sports too, not only Dota 2
        let matches:Vec<Match> = self.session.get_json("/api/sportmatch/Get?categoryID=3693&sportID=2357").unwrap();
        let events = matches.into_iter().filter_map(Into::into).collect();
        
        Ok(events)
    }

    fn make_bet(&self, event: Event, outcome: Outcome, bet: Currency) -> Prime<()> {
        unimplemented!();
    }
}

#[derive(RustcEncodable)]
struct VitalBetAuthData<'a> {
    BrowserFingerPrint: i64,
    Login: &'a str,
    Password: &'a str,
    RememberMe: bool
}

#[derive(RustcDecodable)]
struct Balance {
    Balance: f64
}

#[derive(RustcDecodable)]
struct Matches(Vec<Match>);

#[derive(RustcDecodable)]
struct Match {
    ID: u64,
    // HomeTeamName: String,
    // AwayTeamName: String,
    DateOfMatch: String,
    PreviewOdds: Vec<Odd>
}

#[derive(RustcDecodable)]
struct Odd {
    // isLive: bool,
    Value: f64,
    Title: String,
    // IsSuspended: bool,
    // IsVisible: bool,
    // Status: i32,
    // IsFeedSuspended: bool
}

// TODO(universome): Add some error handling
impl Into<Option<Event>> for Match {
    fn into(self) -> Option<Event> {
        let outcomes = self.PreviewOdds.into_iter().map(|odd| { Outcome(odd.Title, odd.Value) }).collect();
        let date = NaiveDateTime::parse_from_str(&self.DateOfMatch, "%Y-%m-%dT%H:%M:%S").unwrap();

        Some(Event {
            date: DateTime::from_utc(date, UTC),
            kind: Kind::Dota2(Dota2::Series),
            outcomes: outcomes,
            inner_id: self.ID
        })
    }
}
