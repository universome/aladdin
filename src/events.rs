use chrono::NaiveDateTime;

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub date: NaiveDateTime,
    pub game: Game,
    pub kind: Kind,
    pub gamid: u64
}

#[derive(Clone, Debug, PartialEq)]
pub enum Game {
    Dota2
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    OneVsOne {
        team_one: (String, f64),
        team_two: (String, f64),
        draw: Option<f64>
    }
}
