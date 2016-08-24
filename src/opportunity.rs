use events::Outcome;

use self::Strategy::*;

pub enum Strategy { Unbiased, Favorite, Rebel }

pub fn find_best<'a, I>(mut markets: I, strategy: Strategy) -> Option<Vec<(&'a Outcome, f64)>>
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

    let coef = line.iter().fold(0f64, |acc, o| acc + 1./o.1);

    if coef > 1. {
        return None;
    }

    let result = match strategy {
        Unbiased => {
            line.iter()
                .map(|outcome| (*outcome, 1. / line.iter().fold(0., |acc, o| acc + outcome.1/o.1)))
                .collect()
        },

        _ => unimplemented!()
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
fn test_unbiased_none() {
    let markets = [
        [Outcome("X".to_string(), 2.3), Outcome("Y".to_string(), 1.2)],
        [Outcome("X".to_string(), 2.1), Outcome("Y".to_string(), 1.3)]
    ];

    assert_bet!(markets, Unbiased, None);
}
