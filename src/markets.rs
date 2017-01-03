use std::fmt::{Display, Formatter};
use std::fmt::Result as FmtResult;
use time;

pub type OID = u64;

#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    StarCraftBW,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
