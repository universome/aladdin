use std::ops::{Add, Sub, Mul};
use std::convert::From;
use std::fmt::{Display, Formatter, Result as FmtResult};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Currency(pub i64);

impl Display for Currency {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(f, "${}.{:02}", self.0 / 100, self.0 % 100)
    }
}

impl Add for Currency {
    type Output = Currency;

    fn add(self, rhs: Currency) -> Currency {
        Currency(self.0 + rhs.0)
    }
}

impl Sub for Currency {
    type Output = Currency;

    fn sub(self, rhs: Currency) -> Currency {
        Currency(self.0 - rhs.0)
    }
}

macro_rules! impl_int_mul {
    ($num:path) => {
        impl Mul<$num> for Currency {
            type Output = Currency;

            fn mul(self, rhs: $num) -> Currency {
                Currency(self.0 * rhs as i64)
            }
        }

        impl Mul<Currency> for $num {
            type Output = Currency;

            fn mul(self, rhs: Currency) -> Currency {
                Currency(self as i64 * rhs.0)
            }
        }
    }
}

impl_int_mul!(i8);
impl_int_mul!(i32);
impl_int_mul!(i64);
impl_int_mul!(isize);

macro_rules! impl_float_mul {
    ($num:path) => {
        impl Mul<$num> for Currency {
            type Output = Currency;

            fn mul(self, rhs: $num) -> Currency {
                if rhs.is_normal() {
                    Currency((self.0 as $num * rhs).round() as i64)
                } else {
                    Currency(0)
                }
            }
        }

        impl Mul<Currency> for $num {
            type Output = Currency;

            fn mul(self, rhs: Currency) -> Currency {
                if self.is_normal() {
                    Currency((self * rhs.0 as $num).round() as i64)
                } else {
                    Currency(0)
                }
            }
        }
    }
}

impl_float_mul!(f32);
impl_float_mul!(f64);

macro_rules! impl_from {
    ($float:path) => {
        impl From<$float> for Currency {
            fn from(float: $float) -> Currency {
                if float.is_normal() {
                    Currency((float * 100.).round() as i64)
                } else {
                    Currency(0)
                }
            }
        }
    }
}

impl_from!(f32);
impl_from!(f64);

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
    assert_eq!(Currency(2) * 2, Currency(4));
    assert_eq!(Currency(2) * -2, Currency(-4));
    assert_eq!(2 * Currency(2), Currency(4));
    assert_eq!(-2 * Currency(2), Currency(-4));

    assert_eq!(Currency(2) * 2., Currency(4));
    assert_eq!(1.5 * Currency(100), Currency(150));
    assert_eq!(Currency(10) * 1.51, Currency(15));
    assert_eq!(1.58 * Currency(10), Currency(16));
}

#[test]
fn test_convertion() {
    use std::f64;

    assert_eq!(Currency::from(15f32), Currency(1500));
    assert_eq!(Currency::from(15.785f32), Currency(1579));
    assert_eq!(Currency::from(f64::NAN), Currency(0));
    assert_eq!(Currency::from(f64::INFINITY), Currency(0));
}
