#![allow(non_snake_case)]

use kuchiki::NodeRef;

use base::error::Result;
use base::timers::Periodic;
use base::parsing::{NodeRefExt, ElementDataExt};
use base::session::Session;
use base::currency::Currency;
use gamblers::Gambler;
use events::{Offer, Outcome, DRAW, Kind};
use events::kinds::*;

pub struct CybBet {
    session: Session
}

impl CybBet {
    pub fn new() -> CybBet {
        CybBet {
            session: Session::new("https://cybbet.com")
        }
    }
}

impl Gambler for CybBet {
    fn authorize(&self, username: &str, password: &str) -> Result<()> {
        unimplemented!();
    }

    fn check_balance(&self) -> Result<Currency> {
        unimplemented!();
    }

    fn watch(&self, cb: &Fn(Offer, bool)) -> Result<()> {
        let html = try!(self.session.get_html("/"));
        let offers = try!(extract_offers(html));

        for offer in offers {
            cb(offer, true);
        }

        Ok(())
    }

    fn place_bet(&self, offer: Offer, outcome: Outcome, bet: Currency) -> Result<()> {
        unimplemented!();
    }
}

fn extract_offers(html: NodeRef) -> Result<Vec<Offer>> {
    let mut offers = vec![];

    for tr in try!(html.query_all("tr.noResult[data-game-id]")) {
        let trn = tr.as_node();

        let img_game_sprite_class = try!(try!(trn.query(".img_game_sprite")).get_attr("class"));
        let game_class_iter = img_game_sprite_class.split_whitespace();
        let kind_class = try!(game_class_iter.last().ok_or("Empty \"class\" attribute"));

        if try!(trn.query_all(".disabled")).next().is_some() {
            continue;
        }

        let kind = match kind_class {
            "csgo" => Kind::CounterStrike(CounterStrike::Series),
            "dota2" => Kind::Dota2(Dota2::Series),
            "heroesofthestorm" => Kind::HeroesOfTheStorm(HeroesOfTheStorm::Series),
            "lol" => Kind::LeagueOfLegends(LeagueOfLegends::Series),
            "ovw" => Kind::Overwatch(Overwatch::Series),
            "smite" => Kind::Smite(Smite::Series),
            "starcraft2" => Kind::StarCraft2(StarCraft2::Series),
            "wot" => Kind::WorldOfTanks(WorldOfTanks::Series),
            "cod" => continue,
            class => {
                warn!("Unknown kind: {}", class);
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
            Outcome(team_1.trim().to_owned(), try!(coef_1.trim().parse())),
            Outcome(team_2.trim().to_owned(), try!(coef_2.trim().parse()))
        ];

        if let Some(coef_draw) = coef_draw {
            outcomes.push(Outcome(DRAW.to_owned(), try!(coef_draw.trim().parse())));
        }

        offers.push(Offer {
            date: try!(date.parse()),
            kind: kind,
            outcomes: outcomes,
            inner_id: try!(id.parse())
        })
    }

    Ok(offers)
}
