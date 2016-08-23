use chrono::NaiveDateTime;

#[derive(Debug, PartialEq)]
pub struct Event {
    pub date: NaiveDateTime,
    pub kind: Kind,
    pub outcomes: Vec<Outcome>,
    pub inner_id: u64
}

#[derive(Debug, PartialEq)]
pub struct Outcome(pub String, pub f64);

#[derive(Debug, PartialEq)]
pub enum Kind {
    Dota2(Dota2)
}

#[derive(Debug, PartialEq)]
pub enum Dota2 { Series, Map(u32), FirstBlood(u32), First10Kills(u32) }
