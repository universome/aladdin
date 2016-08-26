use std::cmp::{PartialEq, Eq};
use std::hash::{Hash, Hasher};
use chrono::{DateTime, UTC};

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
        if self.date != other.date || self.kind != other.kind {
            return false;
        }

        // Search at least one match (except draw of cause).
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
        self.date.hash(state);
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

#[test]
fn test_fuzzy_eq() {
    assert!(fuzzy_eq("rb", "rb"));
    assert!(fuzzy_eq("rb ", "rb"));
    assert!(fuzzy_eq("RB", "rb"));
    assert!(fuzzy_eq("r.b", "rb"));
    assert!(fuzzy_eq(" r.b", "rb"));
    assert!(fuzzy_eq(" R.8B ", "rb"));
}
