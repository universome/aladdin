use std::cmp::PartialOrd;

use events::Outcome;

use self::Strategy::*;

pub enum Strategy { Unbiased, Favorite, Rebel }

pub struct MarkedOutcome<'a> {
    pub index: usize,
    pub outcome: &'a Outcome,
    pub rate: f64,
    pub profit: f64
}

const LIMIT: usize = 3;

pub fn calc_coef(table: &[&[Outcome]]) -> f64 {
    // Unfortunately, rust doesn't support variable-length array.
    // So, we hardcode the maximum length.
    debug_assert!(table.len() <= LIMIT);

    let mut line = [0.; LIMIT];

    for column in table {
        for (best, outcome) in line.iter_mut().zip(column.iter()) {
            if *best < outcome.1 {
                *best = outcome.1;
            }
        }
    }

    line.iter().map(|x| 1. / x).sum()
}

pub fn find_best<'a>(table: &[&'a [Outcome]], strategy: Strategy) -> Vec<MarkedOutcome<'a>> {
    let len = table.len();

    debug_assert!(len > 0);

    let mut table_iter = table.into_iter();

    let mut line = table_iter.next().unwrap().iter()
        .map(|outcome| MarkedOutcome {
            index: 0,
            outcome: outcome,
            rate: 0.,
            profit: 0.
        })
        .collect::<Vec<_>>();

    for (index, outcomes) in table_iter.enumerate() {
        for (best, outcome) in line.iter_mut().zip(outcomes.iter()) {
            if best.outcome.1 < outcome.1 {
                best.index = index;
                best.outcome = outcome;
            }
        }
    }

    let coef = line.iter().fold(0., |acc, marked| acc + 1. / marked.outcome.1);

    debug_assert!(coef < 1.);

    match strategy {
        Unbiased => {
            for marked in &mut line {
                marked.rate = coef * marked.outcome.1;
                marked.profit = coef - 1.;
            }
        },
        Favorite | Rebel => {
            let mut guess_idx = 0;
            let cmp = if let Favorite = strategy { PartialOrd::ge } else { PartialOrd::le };

            for (idx, marked) in line.iter().enumerate() {
                if cmp(&marked.outcome.1, &line[guess_idx].outcome.1) {
                    guess_idx = idx;
                }
            }

            for marked in &mut line {
                marked.rate = 1. / marked.outcome.1;
                marked.profit = 0.;
            }

            let sum = line.iter().fold(0., |acc, marked| acc + marked.outcome.1);

            let guess = &mut line[guess_idx];

            guess.rate += 1. - sum;
            guess.profit = guess.outcome.1 * guess.rate - sum;
        }
    };

    line
}

//macro_rules! assert_bet {
    //// XXX(loyd): rewrite this shit.
    //($markets:expr, $strategy:expr, $expected:expr) => {
        //let actual = find_best($markets.iter().map(|x| x as &[Outcome]), $strategy)
            //.map(|bets| bets.iter().map(|x| (x.0, (x.1*100.).round()/100.)).collect::<Vec<_>>());

        //assert_eq!(actual, $expected);
    //}
//}

//#[test]
//fn test_none() {
    //let markets = [
        //&[&Outcome("X".to_string(), 2.3), &Outcome("Y".to_string(), 1.2)],
        //&[&Outcome("X".to_string(), 2.1), &Outcome("Y".to_string(), 1.3)]
    //];

    //assert_bet!(markets, Unbiased, None);
    //assert_bet!(markets, Favorite, None);
    //assert_bet!(markets, Rebel, None);
//}

//#[test]
//fn test_unbiased() {
    //let markets = [
        //[Outcome("X".to_string(), 2.3), Outcome("Y".to_string(), 3.2)],
        //[Outcome("X".to_string(), 2.1), Outcome("Y".to_string(), 3.3)]
    //];

    //let x_outcome = Outcome("X".to_string(), 2.3);
    //let y_outcome = Outcome("Y".to_string(), 3.3);
    //let expected = vec![(&x_outcome, 0.59), (&y_outcome, 0.41)];

    //assert_bet!(markets, Unbiased, Some(expected));
//}

//#[test]
//fn test_favorite() {
    //let markets = [
        //[Outcome("X".to_string(), 2.3), Outcome("Y".to_string(), 3.2)],
        //[Outcome("X".to_string(), 2.1), Outcome("Y".to_string(), 3.3)]
    //];

    //let x_outcome = Outcome("X".to_string(), 2.3);
    //let y_outcome = Outcome("Y".to_string(), 3.3);
    //let expected = vec![(&x_outcome, 0.43), (&y_outcome, 0.57)];

    //assert_bet!(markets, Favorite, Some(expected));
//}

//#[test]
//fn test_rebel() {
    //let markets = [
        //[Outcome("X".to_string(), 2.3), Outcome("Y".to_string(), 3.2)],
        //[Outcome("X".to_string(), 2.1), Outcome("Y".to_string(), 3.3)]
    //];

    //let x_outcome = Outcome("X".to_string(), 2.3);
    //let y_outcome = Outcome("Y".to_string(), 3.3);
    //let expected = vec![(&x_outcome, 0.7), (&y_outcome, 0.3)];

    //assert_bet!(markets, Rebel, Some(expected));
//}
