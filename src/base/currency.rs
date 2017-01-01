use std::ops::{Add, Sub, Mul};
use std::convert::From;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::convert::Into;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Currency(pub i64);

impl Display for Currency {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(f, "${}.{:02}", self.0 / 100, self.0 % 100)
    }
}

impl Add for Currency {
    type Output = Currency;

    #[inline]
    fn add(self, rhs: Currency) -> Currency {
        Currency(self.0 + rhs.0)
    }
}

impl Sub for Currency {
    type Output = Currency;

    #[inline]
    fn sub(self, rhs: Currency) -> Currency {
        Currency(self.0 - rhs.0)
    }
}

impl Mul<f64> for Currency {
    type Output = Currency;

    #[inline]
    fn mul(self, rhs: f64) -> Currency {
        if rhs.is_normal() {
            Currency((self.0 as f64 * rhs).round() as i64)
        } else {
            Currency(0)
        }
    }
}

impl Mul<Currency> for f64 {
    type Output = Currency;

    #[inline]
    fn mul(self, rhs: Currency) -> Currency {
        if self.is_normal() {
            Currency((self * rhs.0 as f64).round() as i64)
        } else {
            Currency(0)
        }
    }
}

impl From<f64> for Currency {
    #[inline]
    fn from(float: f64) -> Currency {
        if float.is_normal() {
            Currency((float * 100.).round() as i64)
        } else {
            Currency(0)
        }
    }
}

impl Into<f64> for Currency {
    #[inline]
    fn into(self) -> f64 {
        (self.0 as f64) / 100.
    }
}

#[test]
fn test_addition() {
    assert_eq!(Currency(2) + Currency(3), Currency(5));
    assert_eq!(Currency(2) + Currency(-3), Currency(-1));
}

#[test]
fn test_subtraction() {
    assert_eq!(Currency(2) - Currency(3), Currency(-1));
    assert_eq!(Currency(2) - Currency(-3), Currency(5));
}

#[test]
fn test_multiplication() {
    assert_eq!(Currency(2) * 2., Currency(4));
    assert_eq!(1.5 * Currency(100), Currency(150));
    assert_eq!(Currency(10) * 1.51, Currency(15));
    assert_eq!(1.58 * Currency(10), Currency(16));
}

#[test]
fn test_from_conversion() {
    use std::f64;

    assert_eq!(Currency::from(15.), Currency(1500));
    assert_eq!(Currency::from(15.785), Currency(1579));
    assert_eq!(Currency::from(f64::NAN), Currency(0));
    assert_eq!(Currency::from(f64::INFINITY), Currency(0));
}

#[test]
fn test_into_conversion() {
    let float: f64 = Currency(15).into();

    assert_eq!(float, 0.15);
}
