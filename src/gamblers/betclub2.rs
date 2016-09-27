#![allow(non_snake_case)]

use serde_json as json;

use base::error::{Result};
use base::session::Session;
use base::currency::Currency;
use gamblers::Gambler;
use events::{Offer};
use events::Outcome as Outcome;

pub struct BetClub {
    session: Session
}

impl BetClub {
    pub fn new() -> BetClub {
        BetClub {
            session: Session::new("betclub2.com")
        }
    }
}

impl Gambler for BetClub {
    fn authorize(&self, username: &str, password: &str) -> Result<()> {
        let path = "/WebServices/BRService.asmx/LogIn";
        let request_data = AuthRequest {
            login: username,
            password: password
        };

        self.session.post_json(path, request_data).map(|_| ())
    }

    fn check_balance(&self) -> Result<Currency> {
        let path = "/WebServices/BRService.asmx/GetUserBalance";
        let response = try!(self.session.post_empty_json(path));
        let balance: BalanceResponse = try!(json::from_reader(response));

        Ok(Currency::from(balance.d.Amount))
    }

    fn watch(&self, cb: &Fn(Offer, bool)) -> Result<()> {
        unimplemented!();
    }

    fn place_bet(&self, offer: Offer, outcome: Outcome, stake: Currency) -> Result<()> {
        unimplemented!();
    }
}

#[derive(Serialize, Debug)]
struct AuthRequest<'a> {
    login: &'a str ,
    password: &'a str
}

#[derive(Deserialize, Debug)]
struct BalanceResponse {
    d: Balance
}

#[derive(Deserialize, Debug)]
struct Balance {
    Amount: f64
}
