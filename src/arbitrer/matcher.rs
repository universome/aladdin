use std::char;
use std::iter::FilterMap;
use std::str::Chars;

use markets::{Offer, Game, Kind, Outcome, DRAW};

const UNVALID_TOKENS: &[&str] = &["", "de", "fc", "sc", "fk", "city", "club", "state", "st."];

#[derive(Debug, Clone, Copy)]
struct Token<'a>(&'a str);

type TokenImpl<'a> = FilterMap<Chars<'a>, fn(char) -> Option<char>>;

#[inline]
fn transform(c: char) -> Option<char> {
    if c.is_alphabetic() || c.is_digit(10) {
        c.to_lowercase().next()
    } else {
        None
    }
}

impl<'a> Token<'a> {
    #[inline]
    fn is_abbr(&self) -> bool {
        self.0.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase())
    }

    #[inline]
    fn len(&self) -> usize {
        self.into_iter().count()
    }

    #[inline]
    fn starts_with(&self, other: Token) -> bool {
        let mut other_it = other.into_iter();

        self.into_iter().zip(other_it.by_ref()).all(|(l, r)| l == r) && other_it.next().is_none()
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.into_iter().next().is_none()
    }
}

impl<'a> IntoIterator for Token<'a> {
    type Item = char;
    type IntoIter = TokenImpl<'a>;

    #[inline]
    fn into_iter(self) -> TokenImpl<'a> {
        self.0.chars().filter_map(transform)
    }
}

impl<'a> From<&'a str> for Token<'a> {
    #[inline]
    fn from(word: &str) -> Token {
        Token(word)
    }
}

impl<'a> PartialEq for Token<'a> {
    #[inline]
    fn eq(&self, other: &Token) -> bool {
        self.into_iter().eq(other.into_iter())
    }
}

pub type Headline = (u32, Game, Kind, usize);

#[inline]
pub fn get_headline(offer: &Offer) -> Headline {
    (round_date(offer.date), offer.game, offer.kind, offer.outcomes.len())
}

pub fn compare_offers(left: &Offer, right: &Offer) -> bool {
    debug_assert!(left.outcomes.len() <= 3);
    debug_assert!(right.outcomes.len() <= 3);

    if get_headline(left) != get_headline(right) {
        return false;
    }

    let mut score = 0.;
    let max_score = left.outcomes.iter().filter(|o| o.0 != DRAW).count() as f64;
    let mut reserved = [3; 3];

    // We receive up to 1.0 points for each title.
    for (i, left_outcome) in left.outcomes.iter().filter(|o| o.0 != DRAW).enumerate() {
        let mut max_sim = 0.;
        let mut best_match = 0;

        for (k, right_outcome) in right.outcomes.iter().filter(|o| o.0 != DRAW).enumerate() {
            if reserved.contains(&k) {
                continue;
            }

            let sim = titles_sim(&left_outcome.0, &right_outcome.0);

            if sim >= max_sim {
                max_sim = sim;
                best_match = k;
            }
        }

        reserved[i] = best_match;

        score += max_sim;
    }

    (score / max_score) >= 0.7
}

#[inline]
fn titles_sim(left: &str, right: &str) -> f64 {
    tokens_sim(left, right).max(tokens_sim(right, left))
}

#[inline]
fn coefs_sim(lhs: f64, rhs: f64) -> f64 {
    1. - (lhs - rhs).abs() / (lhs + rhs) // ultra formula :|
}

// Calculates how much tokens from the left string fits to the right one
fn tokens_sim(left: &str, right: &str) -> f64 {
    let mut score = 0.;

    for lhs in get_tokens(left) {
        let mut max_score = 0.0_f64;

        for rhs in get_tokens(right) {
            let score = if lhs == rhs {
                1.
            } else if lhs.len() > 3 && lhs.starts_with(rhs) {
                rhs.len() as f64 / lhs.len() as f64
            } else if lhs.is_abbr() {
                abbreviation_sim(lhs, right)
            } else {
                0.
            };

            max_score = max_score.max(score);
        }

        score += max_score;
    }

    score / get_tokens(left).count() as f64
}

fn get_tokens<'a>(title: &'a str) -> impl Iterator<Item = Token<'a>> {
    title
        .split(|c: char| c.is_whitespace() || c == '-' || c == '/')
        .filter(|s| !UNVALID_TOKENS.contains(&s.to_lowercase().as_str()))
        .map(Token::from)
        .filter(|token| !token.is_empty())
}

fn abbreviation_sim(abbr: Token, title: &str) -> f64 {
    let mut abbr_it = abbr.into_iter();
    let mut letter = abbr_it.next().unwrap();
    let mut matched = 0usize;

    for token in get_tokens(title) {
        let first_char = token.into_iter().next().unwrap();

        if letter == first_char {
            matched += 1;

            letter = match abbr_it.next() {
                Some(c) => c,
                None => break
            };
        }
    }

    matched as f64 / abbr.len() as f64
}

#[inline]
fn round_date(ts: u32) -> u32 {
    (ts + 15 * 60) / (30 * 60) * (30 * 60)
}

// Sorts outcomes according to some etalon offer.
pub fn collate_outcomes<'a>(etalon: &[Outcome], outcomes: &'a [Outcome]) -> Vec<&'a Outcome> {
    let mut result = outcomes.iter().collect::<Vec<_>>();

    for (i, outcome) in etalon.iter().enumerate() {
        let index = i + most_similar_outcome(outcome, &result[i..]);

        result.swap(i, index);
    }

    result
}

// Finds most similar outcome and returns its index in slice.
fn most_similar_outcome(lhs: &Outcome, outcomes: &[&Outcome]) -> usize {
    let mut max_sim = 0.;
    let mut index = 0;

    for (i, rhs) in outcomes.iter().enumerate() {
        let sim = titles_sim(&lhs.0, &rhs.0) * 0.8 + coefs_sim(lhs.1, rhs.1) * 0.2;

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
    use super::{compare_offers, collate_outcomes, titles_sim, round_date, abbreviation_sim, Token};

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

        assert!(!compare_offers(
            &offer!("", 0.0, "", 0.0),
            &offer!("", 0.0, "", 0.0)
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

        assert!(compare_offers(
            &offer!("Northern Iowa", 5.5, "Wichita State", 1.169),
            &offer!("North. Iowa", 5.1, "Wichita St.", 1.17)
        ));
    }

    #[test]
    fn compare_offers_with_similar_beginnings() {
        assert!(!compare_offers(
            &offer!("Alpla HC Hard", 1.76, "A1 Bregenz HB", 2.6, DRAW, 8.4),
            &offer!("Alingsaas HK", 1.1, DRAW, 14.75, "Ricoh HK", 9.25)
        ));

        assert!(!compare_offers(
            &offer!("Alpla Hard", 1.77, "Bregenz", 2.288, DRAW, 11.),
            &offer!("Alingsaas HK", 1.1, DRAW, 14.75, "Ricoh HK", 9.25)
        ));
    }

    #[test]
    fn compare_different_offers() {
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

        assert!(!compare_offers(
            &offer!("Real Betis II", 1.78, "Atletico Espeleno", 4.1, DRAW, 3.4),
            &offer!("Real Sociedad II", 1.5, DRAW, 4.14, "Mensajero", 7.77)
        ));

        assert!(!compare_offers(
            &offer!("Starwings Basket Regio Basel", 5.25, "Benetton Fribourg Olympic", 1.12),
            &offer!("BBC Monthey", 1.08, "BBC Lausanne", 6.74)
        ));

        assert!(!compare_offers(
            &offer!("BBC Monthey", 1.12, "BBC Lausanne", 6.3),
            &offer!("Starwings Basket Regio Basel", 5.25, "Benetton Fribourg Olympic", 1.12)
        ));

        assert!(!compare_offers(
            &offer!("HC La Chaux De Fonds", 1.18, DRAW, 7., "HC Biasca", 8.75),
            &offer!("SCL Tigers", 2.35, DRAW, 4.1, "Lausanne HC", 2.45)
        ));

        assert!(!compare_offers(
            &offer!("HC Lugano", 1.55, DRAW, 4.75, "SC Langenthal", 4.25),
            &offer!("Red Ice Martigny", 2.65, "SC Langenthal", 2.1, DRAW, 4.6)
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

        assert!(!compare_offers(
            &offer!("Kayserispor U21", 2.45, "Karabukspor U21", 2.34, DRAW, 3.8),
            &offer!("Galatasaray] [U21", 1.571, DRAW, 4.0, "Alanyaspor] [U21", 4.75)
        ));

        assert!(!compare_offers(
            &offer!("Altynordu U21", 1.49, "Umraniyespor U21", 5.6, DRAW, 4.),
            &offer!("Mersin Idmanyurdu] [U21", 3.25, DRAW, 3.4, "Bandirmaspor] [U21", 2.),
        ));

        assert!(!compare_offers(
            &offer!("Dover Athletic", 1.49, DRAW, 4.45, "Machine Sazi Tabriz", 6.09),
            &offer!("Dover Athletic", 1.53, DRAW, 4.20, "Maidstone United", 5.50)
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
    fn compare_offers_with_numeric_teams() {
        assert!(compare_offers(
            &offer!("Imperial", 1.12, "8000", 4.25),
            &offer!("Imperial", 1.13, "8000", 4.22)
        ));
    }

    #[test]
    fn compare_offers_with_lowercase() {
        assert!(compare_offers(
            &offer!("Immortals", 1.63, "Mousesports", 2.20),
            &offer!("Immortals", 1.68, "mousesports", 2.19)
        ));

        assert!(compare_offers(
            &offer!("EnvyUs", 1.33, "Tyloo", 3.30),
            &offer!("Envyus", 1.35, "TyLoo", 2.95)
        ));
    }

    #[test]
    fn compare_offers_with_high_coefs() {
        assert!(compare_offers(
            &offer!("Wolfsberger Ac", 18.0, DRAW, 4.15, "FK Austria Wien", 1.25),
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

    #[test]
    fn compare_titles() {
        assert!(titles_sim("HC La Chaux De Fonds", "SCL Tigers") <= 0.3);
    }

    #[test]
    fn compare_abbrs() {
        assert_eq!(abbreviation_sim(Token::from("KL"), "Kek Lol"), 1.);
        assert_eq!(abbreviation_sim(Token::from("KL"), "Kek Shmek Lol"), 1.);
        assert_eq!(abbreviation_sim(Token::from("KKL"), "Kek Lol"), 1./3.);
    }
}
