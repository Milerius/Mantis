//! Hot-path price in venue-specific tick units.

use core::fmt;
use core::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

/// Hot-path price in venue-specific tick units.
///
/// This is a lightweight integer newtype used on the critical path.
/// Conversion to/from fixed-point prices goes through [`InstrumentMeta`](crate::InstrumentMeta).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Ticks(i64);

impl Ticks {
    /// Zero ticks.
    pub const ZERO: Self = Self(0);

    /// Construct from a raw `i64` tick count.
    #[must_use]
    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    /// Extract the raw `i64` tick count.
    #[must_use]
    pub const fn to_raw(self) -> i64 {
        self.0
    }
}

impl Add for Ticks {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl Sub for Ticks {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl Neg for Ticks {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self(-self.0)
    }
}

impl AddAssign for Ticks {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl SubAssign for Ticks {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl Mul<i64> for Ticks {
    type Output = Self;

    fn mul(self, rhs: i64) -> Self::Output {
        Self(self.0 * rhs)
    }
}

impl Div<i64> for Ticks {
    type Output = Self;

    fn div(self, rhs: i64) -> Self::Output {
        Self(self.0 / rhs)
    }
}

impl fmt::Debug for Ticks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Ticks({})", self.0)
    }
}

impl fmt::Display for Ticks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn construction_and_roundtrip() {
        let t = Ticks::from_raw(42);
        assert_eq!(t.to_raw(), 42);
    }

    #[test]
    fn zero() {
        assert_eq!(Ticks::ZERO.to_raw(), 0);
    }

    #[test]
    fn add_sub() {
        let a = Ticks::from_raw(10);
        let b = Ticks::from_raw(3);
        assert_eq!((a + b).to_raw(), 13);
        assert_eq!((a - b).to_raw(), 7);
    }

    #[test]
    fn neg() {
        let t = Ticks::from_raw(5);
        assert_eq!((-t).to_raw(), -5);
    }

    #[test]
    fn assign_ops() {
        let mut t = Ticks::from_raw(10);
        t += Ticks::from_raw(5);
        assert_eq!(t.to_raw(), 15);
        t -= Ticks::from_raw(3);
        assert_eq!(t.to_raw(), 12);
    }

    #[test]
    fn scalar_mul_div() {
        let t = Ticks::from_raw(6);
        assert_eq!((t * 3).to_raw(), 18);
        assert_eq!((t / 2).to_raw(), 3);
    }

    #[test]
    fn ordering() {
        assert!(Ticks::from_raw(1) < Ticks::from_raw(2));
        assert!(Ticks::from_raw(-1) < Ticks::from_raw(0));
    }

    #[test]
    fn display_and_debug() {
        let t = Ticks::from_raw(42);
        assert_eq!(t.to_string(), "42");
        assert_eq!(alloc::format!("{t:?}"), "Ticks(42)");
    }
}
