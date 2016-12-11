use std::hash::{Hash, Hasher};
use std::fmt::{Display, Formatter};
use std::fmt::Result as FmtResult;
use time;

use arbitrer::matcher::round_date;

pub type OID = u64;

#[derive(Debug, Clone)]
pub struct Offer {
    pub oid: OID,
    pub date: u32,
    pub game: Game,
    pub kind: Kind,
    pub outcomes: Vec<Outcome>
}

#[derive(Debug, Clone, PartialEq)]
pub struct Outcome(pub String, pub f64);

pub static DRAW: &str = "(draw)";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Game {
    CounterStrike,
    CrossFire,
    Dota2,
    GearsOfWar,
    Halo,
    Hearthstone,
    HeroesOfTheStorm,
    LeagueOfLegends,
    Overwatch,
    Smite,
    StarCraft2,
    Vainglory,
    WorldOfTanks,
    Fifa,

    Football,
    Tennis,
    Basketball,
    IceHockey,
    Volleyball,
    TableTennis,
    Handball,
    Badminton,
    Baseball,
    Snooker,
    Pool,
    Futsal,
    WaterPolo,
    Rugby,
    Chess,
    Boxing,
    AmericanFootball,
    Bandy,
    Motorsport,
    Biathlon,
    Darts,
    AlpineSkiing,
    SkiJumping,
    Skiing,
    Formula,
    FieldHockey,
    Motorbikes,
    Bowls,
    BicycleRacing,
    Poker,
    Golf,
    Curling,
    Netball,
    MartialArts,
    Cricket,
    Floorball,
    GaelicFootball,
    HorseRacing,
    Hurling
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Kind {
    Series
}

impl Display for Offer {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        let tm = time::at_utc(time::Timespec::new(self.date as i64, 0)).to_local();
        let date = tm.strftime("%d/%m %R").unwrap();

        try!(write!(f, "{} [{:?}] {:?} #{} (", date, self.game, self.kind, self.oid));

        for (idx, outcome) in self.outcomes.iter().enumerate() {
            try!(write!(f, "{}{} x{}", if idx > 0 { "|" } else { "" }, outcome.0, outcome.1));
        }

        write!(f, ")")
    }
}

impl PartialEq for Offer {
    fn eq(&self, other: &Offer) -> bool {
        if self.game != other.game || self.kind != other.kind {
            return false;
        }

        if self.outcomes.len() != other.outcomes.len() {
            return false;
        }

        if round_date(self.date) != round_date(other.date) {
            return false;
        }

        // Search at least one match (except draw of course).
        for fst in &self.outcomes {
            if fst.0 == DRAW { continue; }

            for snd in &other.outcomes {
                if fuzzy_eq(&fst.0, &snd.0) {
                    return true;
                }
            }
        }

        false
    }
}

impl Eq for Offer {}

impl Hash for Offer {
    fn hash<H: Hasher>(&self, state: &mut H) {
        round_date(self.date).hash(state);
        self.game.hash(state);
        self.kind.hash(state);
        self.outcomes.len().hash(state);
    }
}

pub fn fuzzy_eq(lhs: &str, rhs: &str) -> bool {
    let left = lhs.chars().filter(|c| c.is_alphabetic());
    let right = rhs.chars().filter(|c| c.is_alphabetic());

    for (l, r) in left.zip(right) {
        if l.to_lowercase().zip(r.to_lowercase()).any(|(l, r)| l != r) {
            return false;
        }
    }

    true
}

#[test]
fn test_fuzzy_eq() {
    assert!(fuzzy_eq("rb", "rb"));
    assert!(fuzzy_eq("rb ", "rb"));
    assert!(fuzzy_eq("RB", "rb"));
    assert!(fuzzy_eq("r.b", "rb"));
    assert!(fuzzy_eq(" r.b", "rb"));
    assert!(fuzzy_eq(" R.8B ", "rb"));
}
