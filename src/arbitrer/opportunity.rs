use events::Outcome;

use self::Strategy::*;

pub enum Strategy { Unbiased, Favorite, Rebel }

pub struct MarkedOutcome<'a> {
    pub market: usize,
    pub outcome: &'a Outcome,
    pub rate: f64,
    pub profit: f64
}

pub fn calc_margin(table: &[Vec<&Outcome>]) -> f64 {
    debug_assert!(table.len() > 0);

    let mut line = vec![0.; table[0].len()];

    for column in table {
        for (best, outcome) in line.iter_mut().zip(column.iter()) {
            if *best < outcome.1 {
                *best = outcome.1;
            }
        }
    }

    line.iter().map(|x| 1. / x).sum()
}

pub fn find_best<'a>(table: &[Vec<&'a Outcome>], strategy: Strategy) -> Vec<MarkedOutcome<'a>> {
    debug_assert!(table.len() > 0);
    debug_assert!(table[0].len() > 0);

    let mut table_iter = table.into_iter();

    let mut line = table_iter.next().unwrap().iter()
        .map(|outcome| MarkedOutcome {
            market: 0,
            outcome: outcome,
            rate: 0.,
            profit: 0.
        })
        .collect::<Vec<_>>();

    for (index, outcomes) in table_iter.enumerate() {
        debug_assert_eq!(outcomes.len(), table[0].len());

        for (best, outcome) in line.iter_mut().zip(outcomes.iter()) {
            if best.outcome.1 < outcome.1 {
                best.market = index + 1;
                best.outcome = outcome;
            }
        }
    }

    let margin = line.iter().map(|marked| 1. / marked.outcome.1).sum::<f64>();

    debug_assert!(margin < 1.);

    match strategy {
        Unbiased => {
            for marked in &mut line {
                marked.rate = 1. / (margin * marked.outcome.1);
                marked.profit = marked.rate * marked.outcome.1 - 1.;
            }
        },
        Favorite | Rebel => {
            let mut guess_idx = 0;
            let cmp = if let Favorite = strategy { PartialOrd::le } else { PartialOrd::ge };

            for (idx, marked) in line.iter().enumerate() {
                if cmp(&marked.outcome.1, &line[guess_idx].outcome.1) {
                    guess_idx = idx;
                }
            }

            for (idx, marked) in line.iter_mut().enumerate() {
                marked.rate = 1. / marked.outcome.1;

                if idx == guess_idx {
                    marked.rate += 1. - margin;
                }

                marked.profit = marked.rate * marked.outcome.1 - 1.;
            }
        }
    };

    line
}

macro_rules! assert_approx_eq {
    ($lhs:expr, $rhs:expr) => { assert!(($lhs - $rhs).abs() < 0.01) }
}

#[test]
fn test_calc_margin_single() {
    let market = [Outcome("X".to_owned(), 2.3), Outcome("Y".to_owned(), 1.35)];
    let table = [market.iter().collect()];

    assert_approx_eq!(calc_margin(&table), 1.18);
}

#[test]
fn test_calc_margin_multiple() {
    let marked_1 = [Outcome("X".to_owned(), 2.3), Outcome("Y".to_owned(), 1.05)];
    let marked_2 = [Outcome("X".to_owned(), 1.2), Outcome("Y".to_owned(), 1.05)];
    let marked_3 = [Outcome("X".to_owned(), 1.3), Outcome("Y".to_owned(), 1.35)];

    let table = [
        marked_1.iter().collect(),
        marked_2.iter().collect(),
        marked_3.iter().collect()
    ];

    assert_approx_eq!(calc_margin(&table), 1.18);
}

#[test]
fn test_find_best_unbiased() {
    let marked_1 = [Outcome("X".to_owned(), 2.3), Outcome("Y".to_owned(), 1.2)];
    let marked_2 = [Outcome("X".to_owned(), 1.3), Outcome("Y".to_owned(), 1.1)];
    let marked_3 = [Outcome("X".to_owned(), 1.1), Outcome("Y".to_owned(), 3.3)];

    let table = [
        marked_1.iter().collect(),
        marked_2.iter().collect(),
        marked_3.iter().collect()
    ];

    let opp = find_best(&table, Unbiased);

    assert_eq!(opp.len(), 2);
    assert_eq!(opp[0].outcome.0, "X");
    assert_eq!(opp[0].market, 0);
    assert_approx_eq!(opp[0].rate, 0.59);
    assert_approx_eq!(opp[0].profit, 0.36);
    assert_eq!(opp[1].outcome.0, "Y");
    assert_eq!(opp[1].market, 2);
    assert_approx_eq!(opp[1].rate, 0.41);
    assert_approx_eq!(opp[1].profit, 0.36);
}

#[test]
fn test_find_best_favorite() {
    let marked_1 = [Outcome("X".to_owned(), 2.3), Outcome("Y".to_owned(), 1.2)];
    let marked_2 = [Outcome("X".to_owned(), 1.3), Outcome("Y".to_owned(), 1.1)];
    let marked_3 = [Outcome("X".to_owned(), 1.1), Outcome("Y".to_owned(), 3.3)];

    let table = [
        marked_1.iter().collect(),
        marked_2.iter().collect(),
        marked_3.iter().collect()
    ];

    let opp = find_best(&table, Favorite);

    assert_eq!(opp.len(), 2);
    assert_eq!(opp[0].outcome.0, "X");
    assert_eq!(opp[0].market, 0);
    assert_approx_eq!(opp[0].rate, 0.7);
    assert_approx_eq!(opp[0].profit, 0.6);
    assert_eq!(opp[1].outcome.0, "Y");
    assert_eq!(opp[1].market, 2);
    assert_approx_eq!(opp[1].rate, 0.3);
    assert_approx_eq!(opp[1].profit, 0.);
}

#[test]
fn test_find_best_rebel() {
    let marked_1 = [Outcome("X".to_owned(), 2.3), Outcome("Y".to_owned(), 1.2)];
    let marked_2 = [Outcome("X".to_owned(), 1.3), Outcome("Y".to_owned(), 1.1)];
    let marked_3 = [Outcome("X".to_owned(), 1.1), Outcome("Y".to_owned(), 3.3)];

    let table = [
        marked_1.iter().collect(),
        marked_2.iter().collect(),
        marked_3.iter().collect()
    ];

    let opp = find_best(&table, Rebel);

    assert_eq!(opp.len(), 2);
    assert_eq!(opp[0].outcome.0, "X");
    assert_eq!(opp[0].market, 0);
    assert_approx_eq!(opp[0].rate, 0.43);
    assert_approx_eq!(opp[0].profit, 0.);
    assert_eq!(opp[1].outcome.0, "Y");
    assert_eq!(opp[1].market, 2);
    assert_approx_eq!(opp[1].rate, 0.57);
    assert_approx_eq!(opp[1].profit, 0.86);
}
