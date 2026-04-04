//! Probability value with range invariant [0, 1].

use core::fmt;

use mantis_fixed::FixedI64;

/// A probability value in the range [0, 1] with 6 decimal places.
///
/// The raw representation is an integer in `[0, 1_000_000]` (i.e. `FixedI64<6>`
/// in the range `[ZERO, ONE]`). Construction validates this invariant.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Probability(FixedI64<6>);

impl Probability {
    /// Probability of one (certainty).
    pub const ONE: Self = Self(FixedI64::ONE);

    /// Probability of zero (impossibility).
    pub const ZERO: Self = Self(FixedI64::ZERO);

    /// Construct from a `FixedI64<6>`, returning `None` if outside [0, 1].
    #[must_use]
    pub const fn new(value: FixedI64<6>) -> Option<Self> {
        let raw = value.to_raw();
        if raw < 0 || raw > FixedI64::<6>::SCALE {
            None
        } else {
            Some(Self(value))
        }
    }

    /// Construct from a raw `i64`, returning `None` if outside [0, `1_000_000`].
    #[must_use]
    pub const fn from_raw(raw: i64) -> Option<Self> {
        Self::new(FixedI64::from_raw(raw))
    }

    /// Extract the inner `FixedI64<6>`.
    #[must_use]
    pub const fn to_fixed(self) -> FixedI64<6> {
        self.0
    }

    /// The complement: `1 - self`. Always valid because `self` is in [0, 1].
    #[must_use]
    pub const fn complement(self) -> Self {
        // SCALE - raw is always in [0, SCALE] when self is in [0, SCALE].
        Self(FixedI64::from_raw(FixedI64::<6>::SCALE - self.0.to_raw()))
    }

    /// True if the probability is zero.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0.is_zero()
    }
}

impl fmt::Debug for Probability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Probability({})", self.0)
    }
}

impl fmt::Display for Probability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_construction() {
        assert!(Probability::from_raw(0).is_some());
        assert!(Probability::from_raw(500_000).is_some());
        assert!(Probability::from_raw(1_000_000).is_some());
    }

    #[test]
    fn reject_negative() {
        assert!(Probability::from_raw(-1).is_none());
        assert!(Probability::from_raw(-1_000_000).is_none());
    }

    #[test]
    fn reject_above_one() {
        assert!(Probability::from_raw(1_000_001).is_none());
        assert!(Probability::from_raw(2_000_000).is_none());
    }

    #[test]
    fn complement_basic() {
        let p = Probability::from_raw(300_000);
        let Some(p) = p else { return };
        assert_eq!(p.complement().to_fixed().to_raw(), 700_000);
    }

    #[test]
    fn complement_zero() {
        assert_eq!(Probability::ZERO.complement(), Probability::ONE);
    }

    #[test]
    fn complement_one() {
        assert_eq!(Probability::ONE.complement(), Probability::ZERO);
    }

    #[test]
    fn double_complement_identity() {
        let p = Probability::from_raw(420_000);
        let Some(p) = p else { return };
        assert_eq!(p.complement().complement(), p);
    }

    #[test]
    fn is_zero() {
        assert!(Probability::ZERO.is_zero());
        assert!(!Probability::ONE.is_zero());
    }
}
