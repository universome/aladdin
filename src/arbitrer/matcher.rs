use std::char;
use std::iter::Filter;
use std::str::Chars;

use markets::{Offer, Outcome};

const THRESHOLD: f64 = 0.7;

#[derive(Clone, Copy)]
struct Token<'a>(&'a str);

type TokenImpl<'a> = Filter<Chars<'a>, fn(&char) -> bool>;

fn transform(c: &char) -> bool {
    c.is_alphabetic()
}

impl<'a> Token<'a> {
    fn is_abbr(&self) -> bool {
        self.into_iter().all(|c| c.is_uppercase())
    }

    fn len(&self) -> usize {
        self.into_iter().count()
    }

    fn starts_as(&self, another: Token) -> bool {
        self.into_iter().zip(another.into_iter()).all(|(l, r)| l == r)
    }

    fn is_empty(&self) -> bool {
        self.into_iter().next().is_some()
    }
}

impl<'a> IntoIterator for Token<'a> {
    type Item = char;
    type IntoIter = TokenImpl<'a>;

    fn into_iter(self) -> TokenImpl<'a> {
        self.0.chars().filter(transform)
    }
}

impl<'a> From<&'a str> for Token<'a> {
    fn from(word: &str) -> Token {
        Token(word)
    }
}

impl<'a> PartialEq for Token<'a> {
    fn eq(&self, other: &Token) -> bool {
        self.into_iter().eq(other.into_iter())
    }
}

pub fn compare_offers(left: &Offer, right: &Offer) -> bool {
    debug_assert!(left.outcomes.len() <= 3);
    debug_assert!(right.outcomes.len() <= 3);

    if left.game != right.game || left.kind != right.kind
    || left.outcomes.len() != right.outcomes.len()
    || round_date(left.date) != round_date(right.date) {
        return false;
    }

    let mut score = 0.;
    let max_score = left.outcomes.len() as f64;
    let mut reserved = [3; 3];

    // We receive up to 1.0 points for each title.
    for (i, left_outcome) in left.outcomes.iter().enumerate() {
        let mut max_sim = 0.;
        let mut best_match = 0;

        for (k, right_outcome) in right.outcomes.iter().enumerate() {
            if reserved.contains(&k) {
                continue;
            }

            let sim = outcomes_sim(left_outcome, right_outcome);

            if sim >= max_sim {
                max_sim = sim;
                best_match = k;
            }
        }

        reserved[i] = best_match;

        score += max_sim;
    }

    (score / max_score) >= THRESHOLD
}

fn outcomes_sim(lhs: &Outcome, rhs: &Outcome) -> f64 {
    titles_sim(&lhs.0, &rhs.0) * 0.8 + coefs_sim(lhs.1, rhs.1) * 0.2
}

fn coefs_sim(lhs: f64, rhs: f64) -> f64 {
    1. - (lhs - rhs).abs() / (lhs + rhs) // ultra formula :|
}

fn titles_sim(left: &str, right: &str) -> f64 {
    tokens_sim(left, right) * 0.5 + tokens_sim(right, left) * 0.5
}

// Calculates how much tokens from the left string fits to the right one
// TODO(universome): rename it
fn tokens_sim(left: &str, right: &str) -> f64 {
    let mut score = 0.;

    for lhs in get_tokens(left) {
        for rhs in get_tokens(right) {
            if lhs == rhs {
                score += 1.;
            } else if lhs.len() > 3 && lhs.starts_as(rhs) {
                score += 0.9;
            } else if lhs.is_abbr() {
                // Penalize for being an abbreviation.
                score += abbreviation_sim(lhs, right) * 0.7;
            }
        }
    }

    score / (get_tokens(left).count() as f64)
}

// TODO(universome): We can also split CamelCaseWords.
fn get_tokens<'a>(title: &'a str) -> impl Iterator<Item = Token<'a>> {
    title
        .split(|c: char| c.is_whitespace() || c == '-' || c == '/')
        .filter(|s| !s.is_empty() || s != &"FC" || s != &"FK" || s != &"City" || s != &"Club")
        .map(Token::from)
        .filter(|token| token.is_empty())
}

fn abbreviation_sim(abbr: Token, title: &str) -> f64 {
    let mut score = 0.;

    for token in get_tokens(title) {
        for c in abbr {
            if token.into_iter().nth(0).unwrap() == c {
                score += 1.;
            }
        }
    }

    score / (abbr.into_iter().count() as f64)
}

pub fn round_date(ts: u32) -> u32 {
    (ts + 15 * 60) / (30 * 60) * (30 * 60)
}

// Sorts outcomes according to some etalon offer
pub fn collate_outcomes<'a>(etalon: &[Outcome], outcomes: &'a [Outcome]) -> Vec<&'a Outcome> {
    let mut result = outcomes.iter().collect::<Vec<_>>();

    for (i, outcome) in etalon.iter().enumerate() {
        let index = i + most_similar_outcome(outcome, &result[i..]);

        result.swap(i, index);
    }

    result
}

// Finds most similar outcome and returns its index in slice
fn most_similar_outcome(outcome: &Outcome, outcomes: &[&Outcome]) -> usize {
    let mut max_sim = 0.;
    let mut index = 0;

    for (i, o) in outcomes.iter().enumerate() {
        let sim = outcomes_sim(outcome, o);

        if sim > max_sim {
            max_sim = sim;
            index = i;
        }
    }

    index
}

#[cfg(test)]
mod tests {
    use time;

    use markets::{DRAW, Offer, Outcome, Game, Kind};
    use super::*;

    macro_rules! offer {
        ( $( $team_name:expr, $coef:expr ),* ) => { Offer {
            date: 123,
            outcomes: vec![
                $( Outcome($team_name.to_string(), $coef), )*
            ],
            oid: 123, game: Game::Darts, kind: Kind::Series
        }}
    }

    #[test]
    fn compare_primitive_cases() {
        assert!(compare_offers(
            &offer!("kek", 1.31, "lol", 1.31),
            &offer!("kek", 1.31, "lol", 1.31)
        ));
    }

    #[test]
    fn compare_offers_with_extra_tokens() {
        assert!(compare_offers(
            &offer!("San Martin Corrientes", 1.14, "Deportivo Libertad", 5.70),
            &offer!("San Martin de Corrientes", 1.14, "Club Deportivo Libertad", 5.71)
        ));

        assert!(compare_offers(
            &offer!("Belgrano", 1.85, "Sarmiento de Junin", 5., DRAW, 3.),
            &offer!("Belgrano de Cordoba", 1.75, DRAW, 3.34, "Sarmiento", 5.72)
        ));

        assert!(!compare_offers(
            &offer!("Evgueni Chtchetinine", 1.08, "Kiryl Barabanov", 7.3),
            &offer!("Evgueni Chtchetinine (BLR)", 1.54, "David Petr (CZE)", 2.31)
        ));

        assert!(compare_offers(
            &offer!("Portimonense", 1.62, "Braga II", 5.1, DRAW, 3.8),
            &offer!("Portimonense Sc", 1.65, "Sporting Braga B", 4.5, DRAW, 3.8)
        ));

        assert!(compare_offers(
            &offer!("Cruzeiro", 2.2, "Santos", 3.3, DRAW, 3.1),
            &offer!("Cruzeiro Mg", 2.1, DRAW, 3.3, "Santos Sp", 3.4)
        ));

        assert!(compare_offers(
            &offer!("Palmeiras", 1.55, "Botafogo RJ", 5.9, DRAW, 3.8),
            &offer!("Palmeiras Sp", 1.57, DRAW, 3.8, "Botafogo Rj", 5.75)
        ));

        assert!(compare_offers(
            &offer!("Team Tvis Holstebro", 1.4, DRAW, 10., "Liberty Seguros Abc/Uminho", 3.95),
            &offer!("Team Tvis Holstebro", 1.16, DRAW, 11.75, "ABC Braga Uminho", 6.75)
        ));
    }

    #[test]
    fn compare_offers_with_abbrs() {
        assert!(compare_offers(
            &offer!("Gilles Simon", 1.48, "Julien Benneteau", 2.93),
            &offer!("G. Simon", 1.41, "J. Benneteau", 2.74)
        ));

        assert!(compare_offers(
            &offer!("North Carolina Tar Heels", 1.24, "North Carolina State Wolfpack", 4.62),
            &offer!("North Carolina", 1.22, "NC State", 4.6)
        ));

        assert!(compare_offers(
            &offer!("Internazionale Milano", 2.08, "Fiorentina", 3.96, DRAW, 3.58),
            &offer!("Inter Milan", 2.06, DRAW, 3.55, "Fiorentina", 3.79)
        ));
    }

    #[test]
    fn compare_very_different_offers() {
        assert!(!compare_offers(
            &offer!("Deportivo Alaves", 2.62, "Espanyol", 3.16, DRAW, 3.18),
            &offer!("Espanyol B", 2.21, DRAW, 3.32, "Mallorca B", 3.27)
        ));

        assert!(!compare_offers(
            &offer!("Karlstad", 1.7, "Hollvikens", 3.46, DRAW, 5.6),
            &offer!("Iksu] [Women", 1.05, DRAW, 11., "Karlstad IBF] [Women", 13.)
        ));

        assert!(!compare_offers(
            &offer!("Tiro Federal Rosario", 1.833, DRAW, 3.4, "Belgrano Parana", 3.8),
            &offer!("Belgrano de Cordoba", 1.75, DRAW, 3.34, "Sarmiento", 5.72)
        ));

        assert!(!compare_offers(
            &offer!("Sportivo Barracas", 1.7, "Defensores de Cambaceres", 4.6, DRAW, 3.4),
            &offer!("Atletico Camioneros", 1.75, DRAW, 3.4, "Sportivo Barracas Colon", 4.2)
        ));

        assert!(!compare_offers(
            &offer!("MVP.GuMiho", 1.95, "Losira", 1.75),
            &offer!("Losira", 1.16, "RYE.Jieshi", 3.9)
        ));

        assert!(!compare_offers(
            &offer!("AS Roma", 1.67, DRAW, 4.16, "AC Milan", 5.22),
            &offer!("Club Leandro N. Alem", 1.65, "Yupanqui", 5.1, DRAW, 3.4)
        ));
    }

    #[test]
    fn compare_similar_but_different_offers() {
        assert!(!compare_offers(
            &offer!("Sportivo Barracas", 1.7, "Defensores de Cambaceres", 4.6, DRAW, 3.4),
            &offer!("Atletico Camioneros", 1.75, DRAW, 3.4, "Sportivo Barracas Colon", 4.2)
        ));

        assert!(!compare_offers(
            &offer!("Penn State", 1.23, "Michigan State", 4.51),
            &offer!("Ohio State Buckeyes", 1.49, "Michigan Wolverines", 2.845)
        ));
    }

    #[test]
    fn compare_offers_with_very_general_titles() {
        assert!(compare_offers(
            &offer!("Kansas State", 1.02, "Kansas", 17.89),
            &offer!("Kansas State Wildcats", 1.03, "Kansas Jayhawks", 20.)
        ));

        assert!(compare_offers(
            &offer!("Mississippi", 1.27, "Mississippi", 4.03),
            &offer!("Mississippi Rebels", 1.296, "Mississippi State", 3.98)
        ));

        assert!(compare_offers(
            &offer!("Georgia", 1.51, "Georgia Tech", 2.69),
            &offer!("Georgia Bulldogs", 1.54, "Georgia Tech Yellow Jackets", 2.68)
        ));
    }

    #[test]
    fn compare_offers_with_high_coefs() {
        assert!(compare_offers(
            &offer!("Wolfsberger Ac", 18., DRAW, 4.15, "FK Austria Wien", 1.25),
            &offer!("Wolfsberger AC", 2.61, DRAW, 3.28, "Austria Wien", 2.81)
        ));
    }

    #[test]
    fn test_round_date() {
        fn to_unix(time: &str) -> u32 {
            let date = "2016-08-28 ".to_owned() + time;
            time::strptime(&date, "%F %H:%M").unwrap().to_timespec().sec as u32
        }

        assert_eq!(round_date(to_unix("11:30")), to_unix("11:30"));
        assert_eq!(round_date(to_unix("11:44")), to_unix("11:30"));
        assert_eq!(round_date(to_unix("11:45")), to_unix("12:00"));
        assert_eq!(round_date(to_unix("12:00")), to_unix("12:00"));
        assert_eq!(round_date(to_unix("12:14")), to_unix("12:00"));
        assert_eq!(round_date(to_unix("12:15")), to_unix("12:30"));
        assert_eq!(round_date(to_unix("12:30")), to_unix("12:30"));
    }

    #[test]
    fn test_collate_outcomes() {
        assert_eq!(
            collate_outcomes(
                &[
                    Outcome("Wolfsberger Ac".to_string(), 18.),
                    Outcome(DRAW.to_string(), 4.15),
                    Outcome("FK Austria Wien".to_string(), 1.25)
                ],
                &[
                    Outcome("Wolfsberger AC".to_string(), 2.61),
                    Outcome(DRAW.to_string(), 3.28),
                    Outcome("Austria Wien".to_string(), 2.81)
                ]
            ),
            vec![
                &Outcome("Wolfsberger AC".to_string(), 2.61),
                &Outcome(DRAW.to_string(), 3.28),
                &Outcome("Austria Wien".to_string(), 2.81)
            ]
        );

        assert_eq!(
            collate_outcomes(
                &[
                    Outcome("Kansas".to_string(), 17.89),
                    Outcome("Kansas State".to_string(), 1.02)
                ],
                &[
                    Outcome("Kansas State Wildcats".to_string(), 1.03),
                    Outcome("Kansas Jayhawks".to_string(), 20.)
                ]
            ),
            vec![
                &Outcome("Kansas Jayhawks".to_string(), 20.),
                &Outcome("Kansas State Wildcats".to_string(), 1.03)
            ]
        );

        assert_eq!(
            collate_outcomes(
                &[
                    Outcome("Mississippi State".to_string(), 3.98),
                    Outcome("Mississippi Rebels".to_string(), 1.296)
                ],
                &[
                    Outcome("Mississippi".to_string(), 1.27),
                    Outcome("Mississippi".to_string(), 4.03)
                ]
            ),
            vec![
                &Outcome("Mississippi".to_string(), 4.03),
                &Outcome("Mississippi".to_string(), 1.27)
            ]
        );
    }
}