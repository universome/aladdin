use chrono::NaiveDateTime;

#[derive(Debug, PartialEq)]
pub struct Event {
    pub date: NaiveDateTime,
    pub kind: Kind,
    pub odds: Odds,
    pub gamid: u64
}

#[derive(Debug, PartialEq)]
pub enum Kind {
    Dota2(Dota2)
}

#[derive(Debug, PartialEq)]
pub enum Odds {
    Certain {
        first: (String, f64),
        second: (String, f64)
    },
    Uncertain {
        first: (String, f64),
        second: (String, f64),
        draw: f64
    }
}

#[derive(Debug, PartialEq)]
pub enum Dota2 { Series, Map(u32), FirstBlood(u32), First10Kills(u32) }
