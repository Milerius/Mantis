//! Add, Sub, Neg operators and checked/saturating/wrapping variants for `FixedI64`.

use core::ops::{Add, AddAssign, Neg, Sub, SubAssign};

use crate::FixedI64;

// ---------------------------------------------------------------------------
// Operator trait implementations
// ---------------------------------------------------------------------------

impl<const D: u8> Add for FixedI64<D> {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::from_raw(self.to_raw() + rhs.to_raw())
    }
}

impl<const D: u8> Sub for FixedI64<D> {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::from_raw(self.to_raw() - rhs.to_raw())
    }
}

impl<const D: u8> Neg for FixedI64<D> {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self::from_raw(-self.to_raw())
    }
}

impl<const D: u8> AddAssign for FixedI64<D> {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl<const D: u8> SubAssign for FixedI64<D> {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

// ---------------------------------------------------------------------------
// Checked / saturating / wrapping methods
// ---------------------------------------------------------------------------

impl<const D: u8> FixedI64<D> {
    /// Checked addition. Returns `None` on overflow.
    #[must_use]
    pub const fn checked_add(self, rhs: Self) -> Option<Self> {
        match self.to_raw().checked_add(rhs.to_raw()) {
            Some(v) => Some(Self::from_raw(v)),
            None => None,
        }
    }

    /// Checked subtraction. Returns `None` on overflow.
    #[must_use]
    pub const fn checked_sub(self, rhs: Self) -> Option<Self> {
        match self.to_raw().checked_sub(rhs.to_raw()) {
            Some(v) => Some(Self::from_raw(v)),
            None => None,
        }
    }

    /// Checked negation. Returns `None` for `MIN` (which has no positive counterpart).
    #[must_use]
    pub const fn checked_neg(self) -> Option<Self> {
        match self.to_raw().checked_neg() {
            Some(v) => Some(Self::from_raw(v)),
            None => None,
        }
    }

    /// Saturating addition. Clamps at `MAX` or `MIN` on overflow.
    #[must_use]
    pub const fn saturating_add(self, rhs: Self) -> Self {
        Self::from_raw(self.to_raw().saturating_add(rhs.to_raw()))
    }

    /// Saturating subtraction. Clamps at `MAX` or `MIN` on overflow.
    #[must_use]
    pub const fn saturating_sub(self, rhs: Self) -> Self {
        Self::from_raw(self.to_raw().saturating_sub(rhs.to_raw()))
    }

    /// Wrapping addition. Wraps around on overflow.
    #[must_use]
    pub const fn wrapping_add(self, rhs: Self) -> Self {
        Self::from_raw(self.to_raw().wrapping_add(rhs.to_raw()))
    }

    /// Wrapping subtraction. Wraps around on overflow.
    #[must_use]
    pub const fn wrapping_sub(self, rhs: Self) -> Self {
        Self::from_raw(self.to_raw().wrapping_sub(rhs.to_raw()))
    }

    /// Wrapping negation. Wraps `MIN` to `MIN`.
    #[must_use]
    pub const fn wrapping_neg(self) -> Self {
        Self::from_raw(self.to_raw().wrapping_neg())
    }

    /// Checked absolute value. Returns `None` for `MIN`.
    #[must_use]
    pub const fn checked_abs(self) -> Option<Self> {
        match self.to_raw().checked_abs() {
            Some(v) => Some(Self::from_raw(v)),
            None => None,
        }
    }

    /// Saturating absolute value. Clamps `MIN` to `MAX`.
    #[must_use]
    pub const fn saturating_abs(self) -> Self {
        Self::from_raw(self.to_raw().saturating_abs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type F6 = FixedI64<6>;

    #[test]
    fn add_basic() {
        let a = F6::from_raw(1_500_000); // 1.5
        let b = F6::from_raw(2_500_000); // 2.5
        assert_eq!((a + b).to_raw(), 4_000_000);
    }

    #[test]
    fn sub_basic() {
        let a = F6::from_raw(3_000_000);
        let b = F6::from_raw(1_500_000);
        assert_eq!((a - b).to_raw(), 1_500_000);
    }

    #[test]
    fn neg_basic() {
        let a = F6::from_raw(1_500_000);
        assert_eq!((-a).to_raw(), -1_500_000);
    }

    #[test]
    fn neg_negative() {
        let a = F6::from_raw(-1_500_000);
        assert_eq!((-a).to_raw(), 1_500_000);
    }

    #[test]
    fn add_assign() {
        let mut a = F6::from_raw(1_000_000);
        a += F6::from_raw(500_000);
        assert_eq!(a.to_raw(), 1_500_000);
    }

    #[test]
    fn sub_assign() {
        let mut a = F6::from_raw(2_000_000);
        a -= F6::from_raw(500_000);
        assert_eq!(a.to_raw(), 1_500_000);
    }

    #[test]
    fn checked_add_normal() {
        let a = F6::from_raw(1_000_000);
        let b = F6::from_raw(2_000_000);
        assert_eq!(a.checked_add(b).map(FixedI64::to_raw), Some(3_000_000));
    }

    #[test]
    fn checked_add_overflow() {
        let a = F6::MAX;
        let b = F6::from_raw(1);
        assert!(a.checked_add(b).is_none());
    }

    #[test]
    fn checked_sub_normal() {
        let a = F6::from_raw(3_000_000);
        let b = F6::from_raw(1_000_000);
        assert_eq!(a.checked_sub(b).map(FixedI64::to_raw), Some(2_000_000));
    }

    #[test]
    fn checked_sub_overflow() {
        let a = F6::MIN;
        let b = F6::from_raw(1);
        assert!(a.checked_sub(b).is_none());
    }

    #[test]
    fn checked_neg_normal() {
        let a = F6::from_raw(1_000_000);
        assert_eq!(a.checked_neg().map(FixedI64::to_raw), Some(-1_000_000));
    }

    #[test]
    fn checked_neg_min_overflows() {
        assert!(F6::MIN.checked_neg().is_none());
    }

    #[test]
    fn saturating_add_at_max() {
        let a = F6::MAX;
        let b = F6::from_raw(1);
        assert_eq!(a.saturating_add(b), F6::MAX);
    }

    #[test]
    fn saturating_sub_at_min() {
        let a = F6::MIN;
        let b = F6::from_raw(1);
        assert_eq!(a.saturating_sub(b), F6::MIN);
    }

    #[test]
    fn wrapping_add_wraps() {
        let a = F6::MAX;
        let b = F6::from_raw(1);
        assert_eq!(a.wrapping_add(b), F6::MIN);
    }

    #[test]
    fn wrapping_sub_wraps() {
        let a = F6::MIN;
        let b = F6::from_raw(1);
        assert_eq!(a.wrapping_sub(b), F6::MAX);
    }

    #[test]
    fn wrapping_neg_of_min() {
        // i64::MIN.wrapping_neg() == i64::MIN
        assert_eq!(F6::MIN.wrapping_neg(), F6::MIN);
    }

    #[test]
    fn checked_abs_positive() {
        let a = F6::from_raw(1_500_000);
        assert_eq!(a.checked_abs().map(FixedI64::to_raw), Some(1_500_000));
    }

    #[test]
    fn checked_abs_negative() {
        let a = F6::from_raw(-1_500_000);
        assert_eq!(a.checked_abs().map(FixedI64::to_raw), Some(1_500_000));
    }

    #[test]
    fn checked_abs_min_overflows() {
        assert!(F6::MIN.checked_abs().is_none());
    }

    #[test]
    fn saturating_abs_min_clamps_to_max() {
        assert_eq!(F6::MIN.saturating_abs(), F6::MAX);
    }
}
