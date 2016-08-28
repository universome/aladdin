use std::hash::{Hash, Hasher};

#[derive(Debug, Clone)]
pub struct Offer {
    pub date: u32,
    pub kind: Kind,
    pub outcomes: Vec<Outcome>,
    pub inner_id: u64
}

#[derive(Debug, Clone, PartialEq)]
pub struct Outcome(pub String, pub f64);

pub static DRAW: &'static str = "(draw)";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Kind {
    Dota2(Dota2)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Dota2 { Series, Map(u32), FirstBlood(u32), First10Kills(u32) }

impl PartialEq for Offer {
    fn eq(&self, other: &Offer) -> bool {
        if self.kind != other.kind {
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
        self.kind.hash(state);
    }
}

fn fuzzy_eq(lhs: &str, rhs: &str) -> bool {
    let left = lhs.chars().filter(|c| c.is_alphabetic());
    let right = rhs.chars().filter(|c| c.is_alphabetic());

    for (l, r) in left.zip(right) {
        if l.to_lowercase().zip(r.to_lowercase()).any(|(l, r)| l != r) {
            return false;
        }
    }

    true
}

fn round_date(ts: u32) -> u32 {
    ts / (30 * 60) * (30 * 60)
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

#[test]
fn test_round_ts() {
    use time::strptime;

    fn to_unix(time: &str) -> u32 {
        let date = "2016-08-28 ".to_owned() + time;
        strptime(&date, "%F %H:%M").unwrap().to_timespec().sec as u32
    }

    assert_eq!(round_date(to_unix("12:00")), to_unix("12:00"));
    assert_eq!(round_date(to_unix("12:29")), to_unix("12:00"));
    assert_eq!(round_date(to_unix("12:30")), to_unix("12:30"));
    assert_eq!(round_date(to_unix("12:31")), to_unix("12:30"));
    assert_eq!(round_date(to_unix("12:59")), to_unix("12:30"));
}
