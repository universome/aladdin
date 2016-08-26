use std::cmp::{PartialEq, Eq};
use std::hash::{Hash, Hasher};
use chrono::{DateTime, UTC, TimeZone};

#[derive(Debug, Clone)]
pub struct Offer {
    pub date: DateTime<UTC>,
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
        if round_ts(self.date.timestamp()) != round_ts(other.date.timestamp()) {
            return false;
        }

        if self.kind != other.kind {
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
        round_ts(self.date.timestamp()).hash(state);
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

fn round_ts(ts: i64) -> i64 {
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
    fn assert_ts(inp_h: u32, inp_m: u32, exp_h: u32, exp_m: u32) {
        assert_eq!(round_ts(UTC.ymd(2016, 8, 26).and_hms(inp_h, inp_m, 0).timestamp()),
                   UTC.ymd(2016, 8, 26).and_hms(exp_h, exp_m, 0).timestamp());
    }

    assert_ts(12, 0, 12, 0);
    assert_ts(12, 29, 12, 0);
    assert_ts(12, 30, 12, 30);
    assert_ts(12, 31, 12, 30);
    assert_ts(12, 59, 12, 30);
}
