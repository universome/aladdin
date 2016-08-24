use std::cmp::PartialOrd;

use events::Outcome;

use self::Strategy::*;

pub enum Strategy { Unbiased, Favorite, Rebel }

pub fn find_best<'a, I>(markets: I, strategy: Strategy) -> Option<Vec<(&'a Outcome, f64)>>
    where I: IntoIterator<Item = &'a [Outcome]>
{
    let mut markets = markets.into_iter();
    let mut line;

    if let Some(outcomes) = markets.next() {
        line = outcomes.iter().collect::<Vec<_>>();
    } else {
        return None;
    }

    for outcomes in markets {
        for (best, outcome) in line.iter_mut().zip(outcomes.iter()) {
            if outcome.1 > best.1 {
                *best = outcome;
            }
        }
    }

    let coef = line.iter().fold(0., |acc, o| acc + 1. / o.1);

    if coef > 1. {
        return None;
    }

    let result = match strategy {
        Unbiased => {
            line.iter()
                .map(|outcome| {
                    let sum = line.iter().fold(0., |acc, o| acc + outcome.1 / o.1);
                    (*outcome, 1. / sum)
                })
                .collect()
        },
        Favorite | Rebel => {
            let mut guess_idx = 0;
            let cmp = if let Favorite = strategy { PartialOrd::ge } else { PartialOrd::le };

            for (idx, outcome) in line.iter().enumerate() {
                if cmp(&outcome.1, &line[guess_idx].1) {
                    guess_idx = idx;
                }
            }

            let mut result = line.iter().map(|o| (*o, 1. / o.1)).collect::<Vec<_>>();
            let sum = result.iter().fold(0., |a, o| a + o.1);

            result[guess_idx].1 += 1. - sum;
            result
        }
    };

    Some(result)
}

macro_rules! assert_bet {
    // XXX(loyd): rewrite this shit.
    ($markets:expr, $strategy:expr, $expected:expr) => {
        let actual = find_best($markets.iter().map(|x| x as &[Outcome]), $strategy)
            .map(|bets| bets.iter().map(|x| (x.0, (x.1*100.).round()/100.)).collect::<Vec<_>>());

        assert_eq!(actual, $expected);
    }
}

#[test]
fn test_none() {
    let markets = [
        [Outcome("X".to_string(), 2.3), Outcome("Y".to_string(), 1.2)],
        [Outcome("X".to_string(), 2.1), Outcome("Y".to_string(), 1.3)]
    ];

    assert_bet!(markets, Unbiased, None);
    assert_bet!(markets, Favorite, None);
    assert_bet!(markets, Rebel, None);
}

#[test]
fn test_unbiased() {
    let markets = [
        [Outcome("X".to_string(), 2.3), Outcome("Y".to_string(), 3.2)],
        [Outcome("X".to_string(), 2.1), Outcome("Y".to_string(), 3.3)]
    ];

    let x_outcome = Outcome("X".to_string(), 2.3);
    let y_outcome = Outcome("Y".to_string(), 3.3);
    let expected = vec![(&x_outcome, 0.59), (&y_outcome, 0.41)];

    assert_bet!(markets, Unbiased, Some(expected));
}

#[test]
fn test_favorite() {
    let markets = [
        [Outcome("X".to_string(), 2.3), Outcome("Y".to_string(), 3.2)],
        [Outcome("X".to_string(), 2.1), Outcome("Y".to_string(), 3.3)]
    ];

    let x_outcome = Outcome("X".to_string(), 2.3);
    let y_outcome = Outcome("Y".to_string(), 3.3);
    let expected = vec![(&x_outcome, 0.43), (&y_outcome, 0.57)];

    assert_bet!(markets, Favorite, Some(expected));
}

#[test]
fn test_rebel() {
    let markets = [
        [Outcome("X".to_string(), 2.3), Outcome("Y".to_string(), 3.2)],
        [Outcome("X".to_string(), 2.1), Outcome("Y".to_string(), 3.3)]
    ];

    let x_outcome = Outcome("X".to_string(), 2.3);
    let y_outcome = Outcome("Y".to_string(), 3.3);
    let expected = vec![(&x_outcome, 0.7), (&y_outcome, 0.3)];

    assert_bet!(markets, Rebel, Some(expected));
}
