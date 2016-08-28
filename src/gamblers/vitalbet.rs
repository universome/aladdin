#![allow(non_snake_case)]

use time;

use base::error::Result;
use base::timers::Periodic;
use base::session::Session;
use base::currency::Currency;
use gamblers::Gambler;
use events::{Offer, Outcome, Kind, Dota2};

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
    fn authorize(&self, username: &str, password: &str) -> Result<()> {
        let body = VitalBetAuthData {
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
        // TODO(universome): removing offers.
        // TODO(universome): optimize this.
        for _ in Periodic::new(40) {
            // TODO(universome): we should get offers from other sports too, not only Dota 2.
            let path = "/api/sportmatch/Get?categoryID=3693&sportID=2357";
            let matches = try!(self.session.get_json::<Vec<Match>>(path));

            for match_ in matches {
                if let Some(offer) = try!(match_.into()) {
                    cb(offer, true)
                }
            }
        }

        Ok(())
    }

    fn place_bet(&self, offer: Offer, outcome: Outcome, bet: Currency) -> Result<()> {
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
struct Match {
    ID: u64,
    DateOfMatch: String,
    PreviewOdds: Vec<Odd>
}

#[derive(RustcDecodable)]
struct Odd {
    IsSuspended: bool,
    Value: f64,
    Title: String
}

impl Into<Result<Option<Offer>>> for Match {
    fn into(self) -> Result<Option<Offer>> {
        let outcomes = self.PreviewOdds.into_iter()
            .filter(|odd| !odd.IsSuspended)
            .map(|odd| Outcome(odd.Title, odd.Value))
            .collect();

        let ts = try!(time::strptime(&self.DateOfMatch, "%Y-%m-%dT%H:%M:%S")).to_timespec();

        Ok(Some(Offer {
            date: ts.sec as u32,
            kind: Kind::Dota2(Dota2::Series),
            outcomes: outcomes,
            inner_id: self.ID
        }))
    }
}
