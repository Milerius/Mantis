//! Signed inventory position in venue-specific lot units.

use core::fmt;
use core::ops::{Add, AddAssign, Neg, Sub, SubAssign};

use crate::Lots;

/// Signed inventory position in venue-specific lot units.
///
/// Positive values represent long positions, negative values represent short
/// positions. Use [`Lots`] for unsigned (order quantity) contexts.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct SignedLots(i64);

impl SignedLots {
    /// Zero position.
    pub const ZERO: Self = Self(0);

    /// Construct from a raw `i64` lot count.
    #[must_use]
    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    /// Extract the raw `i64` lot count.
    #[must_use]
    pub const fn to_raw(self) -> i64 {
        self.0
    }

    /// Return `true` if the position is zero.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// Return the sign of the position: `1` for long, `-1` for short, `0` for flat.
    #[must_use]
    pub const fn signum(self) -> i64 {
        self.0.signum()
    }

    /// Return the absolute lot count as a [`SignedLots`].
    #[must_use]
    pub const fn abs(self) -> Self {
        Self(self.0.abs())
    }
}

impl From<Lots> for SignedLots {
    fn from(lots: Lots) -> Self {
        Self(lots.to_raw())
    }
}

impl Add for SignedLots {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl Sub for SignedLots {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl Neg for SignedLots {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self(-self.0)
    }
}

impl AddAssign for SignedLots {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl SubAssign for SignedLots {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl Default for SignedLots {
    fn default() -> Self {
        Self::ZERO
    }
}

impl fmt::Debug for SignedLots {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SignedLots({})", self.0)
    }
}

impl fmt::Display for SignedLots {
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
        let s = SignedLots::from_raw(42);
        assert_eq!(s.to_raw(), 42);
    }

    #[test]
    fn zero() {
        assert_eq!(SignedLots::ZERO.to_raw(), 0);
        assert!(SignedLots::ZERO.is_zero());
        assert!(!SignedLots::from_raw(1).is_zero());
    }

    #[test]
    fn add_sub() {
        let a = SignedLots::from_raw(10);
        let b = SignedLots::from_raw(3);
        assert_eq!((a + b).to_raw(), 13);
        assert_eq!((a - b).to_raw(), 7);
    }

    #[test]
    fn neg() {
        let s = SignedLots::from_raw(5);
        assert_eq!((-s).to_raw(), -5);
        let short = SignedLots::from_raw(-3);
        assert_eq!((-short).to_raw(), 3);
    }

    #[test]
    fn assign_ops() {
        let mut s = SignedLots::from_raw(10);
        s += SignedLots::from_raw(5);
        assert_eq!(s.to_raw(), 15);
        s -= SignedLots::from_raw(3);
        assert_eq!(s.to_raw(), 12);
    }

    #[test]
    fn signum() {
        assert_eq!(SignedLots::from_raw(7).signum(), 1);
        assert_eq!(SignedLots::from_raw(-7).signum(), -1);
        assert_eq!(SignedLots::ZERO.signum(), 0);
    }

    #[test]
    fn abs() {
        assert_eq!(SignedLots::from_raw(-5).abs().to_raw(), 5);
        assert_eq!(SignedLots::from_raw(5).abs().to_raw(), 5);
        assert_eq!(SignedLots::ZERO.abs().to_raw(), 0);
    }

    #[test]
    fn from_lots() {
        let lots = Lots::from_raw(99);
        let signed: SignedLots = lots.into();
        assert_eq!(signed.to_raw(), 99);
    }

    #[test]
    fn ordering() {
        assert!(SignedLots::from_raw(-1) < SignedLots::from_raw(0));
        assert!(SignedLots::from_raw(0) < SignedLots::from_raw(1));
    }

    #[test]
    fn display_and_debug() {
        let s = SignedLots::from_raw(42);
        assert_eq!(s.to_string(), "42");
        assert_eq!(alloc::format!("{s:?}"), "SignedLots(42)");

        let short = SignedLots::from_raw(-7);
        assert_eq!(short.to_string(), "-7");
        assert_eq!(alloc::format!("{short:?}"), "SignedLots(-7)");
    }

    #[test]
    fn default_is_zero() {
        assert_eq!(SignedLots::default(), SignedLots::ZERO);
    }
}
