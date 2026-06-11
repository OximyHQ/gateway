//! Integer-only USD. Unit: micro-dollars (µUSD); 1 USD = 1_000_000 µUSD.
//! Floats are never used for money — only `from_dollars_f64`/`as_dollars_f64`
//! exist for display and test ergonomics, never for accumulation.

use std::iter::Sum;
use std::ops::{Add, AddAssign, Sub, SubAssign};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct Usd(i64);

impl Usd {
    pub const ZERO: Usd = Usd(0);

    pub const fn from_micros(micros: i64) -> Self {
        Usd(micros)
    }

    pub const fn micros(self) -> i64 {
        self.0
    }

    /// For tests/display only. Rounds to the nearest µUSD.
    pub fn from_dollars_f64(dollars: f64) -> Self {
        Usd((dollars * 1_000_000.0).round() as i64)
    }

    pub fn as_dollars_f64(self) -> f64 {
        self.0 as f64 / 1_000_000.0
    }
}

impl Add for Usd {
    type Output = Usd;
    fn add(self, rhs: Usd) -> Usd {
        Usd(self.0 + rhs.0)
    }
}
impl Sub for Usd {
    type Output = Usd;
    fn sub(self, rhs: Usd) -> Usd {
        Usd(self.0 - rhs.0)
    }
}
impl AddAssign for Usd {
    fn add_assign(&mut self, rhs: Usd) {
        self.0 += rhs.0;
    }
}
impl SubAssign for Usd {
    fn sub_assign(&mut self, rhs: Usd) {
        self.0 -= rhs.0;
    }
}
impl Sum for Usd {
    fn sum<I: Iterator<Item = Usd>>(iter: I) -> Usd {
        Usd(iter.map(|u| u.0).sum())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn micros_roundtrip() {
        let one_dollar = Usd::from_micros(1_000_000);
        assert_eq!(one_dollar.micros(), 1_000_000);
        assert_eq!(one_dollar.as_dollars_f64(), 1.0);
    }

    #[test]
    fn arithmetic_is_exact() {
        let mut total = Usd::ZERO;
        for _ in 0..3 {
            total += Usd::from_micros(100_000); // $0.10 each
        }
        assert_eq!(total, Usd::from_micros(300_000)); // exactly $0.30
        assert_eq!(total - Usd::from_micros(50_000), Usd::from_micros(250_000));
    }

    #[test]
    fn sum_iterator() {
        let v = [
            Usd::from_micros(1),
            Usd::from_micros(2),
            Usd::from_micros(3),
        ];
        let s: Usd = v.into_iter().sum();
        assert_eq!(s, Usd::from_micros(6));
    }

    #[test]
    fn ordering() {
        assert!(Usd::from_micros(1) < Usd::from_micros(2));
        assert!(Usd::ZERO < Usd::from_micros(1));
    }
}
