//! USDC amount semantic wrapper over `FixedI64<6>`.

use core::fmt;
use core::ops::{Add, AddAssign, Neg, Sub, SubAssign};

use mantis_fixed::FixedI64;

/// USDC amount with 6 decimal places of precision.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct UsdcAmount(FixedI64<6>);

impl UsdcAmount {
    /// Zero USDC.
    pub const ZERO: Self = Self(FixedI64::ZERO);

    /// Construct from a `FixedI64<6>`.
    #[must_use]
    pub const fn from_fixed(inner: FixedI64<6>) -> Self {
        Self(inner)
    }

    /// Extract the inner `FixedI64<6>`.
    #[must_use]
    pub const fn to_fixed(self) -> FixedI64<6> {
        self.0
    }

    /// Construct from a pre-scaled raw integer.
    #[must_use]
    pub const fn from_raw(raw: i64) -> Self {
        Self(FixedI64::from_raw(raw))
    }

    /// Checked addition. Returns `None` on overflow.
    #[must_use]
    pub const fn checked_add(self, rhs: Self) -> Option<Self> {
        match self.0.checked_add(rhs.0) {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }

    /// Checked subtraction. Returns `None` on overflow.
    #[must_use]
    pub const fn checked_sub(self, rhs: Self) -> Option<Self> {
        match self.0.checked_sub(rhs.0) {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }

    /// Checked negation. Returns `None` for the minimum value.
    #[must_use]
    pub const fn checked_neg(self) -> Option<Self> {
        match self.0.checked_neg() {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }

    /// Checked multiplication by an integer scalar.
    #[must_use]
    pub const fn checked_mul_int(self, rhs: i64) -> Option<Self> {
        match self.0.checked_mul_int(rhs) {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }

    /// Checked division by an integer scalar.
    #[must_use]
    pub const fn checked_div_int(self, rhs: i64) -> Option<Self> {
        match self.0.checked_div_int(rhs) {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }

    /// Checked absolute value. Returns `None` for the minimum value.
    #[must_use]
    pub const fn checked_abs(self) -> Option<Self> {
        match self.0.checked_abs() {
            Some(v) => Some(Self(v)),
            None => None,
        }
    }

    /// True if this amount is zero.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0.is_zero()
    }
}

impl Add for UsdcAmount {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl Sub for UsdcAmount {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl Neg for UsdcAmount {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self(-self.0)
    }
}

impl AddAssign for UsdcAmount {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl SubAssign for UsdcAmount {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl fmt::Debug for UsdcAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "UsdcAmount({})", self.0)
    }
}

impl fmt::Display for UsdcAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero() {
        assert!(UsdcAmount::ZERO.is_zero());
    }

    #[test]
    fn add_sub() {
        let a = UsdcAmount::from_raw(1_500_000); // 1.5
        let b = UsdcAmount::from_raw(2_500_000); // 2.5
        assert_eq!((a + b).to_fixed().to_raw(), 4_000_000);
        assert_eq!((b - a).to_fixed().to_raw(), 1_000_000);
    }

    #[test]
    fn neg() {
        let a = UsdcAmount::from_raw(1_000_000);
        assert_eq!((-a).to_fixed().to_raw(), -1_000_000);
    }

    #[test]
    fn scalar_ops() {
        let a = UsdcAmount::from_raw(2_000_000); // 2.0
        assert_eq!(
            a.checked_mul_int(3).map(|x| x.to_fixed().to_raw()),
            Some(6_000_000)
        );
        assert_eq!(
            a.checked_div_int(2).map(|x| x.to_fixed().to_raw()),
            Some(1_000_000)
        );
    }

    #[test]
    fn checked_abs() {
        let neg = UsdcAmount::from_raw(-1_500_000);
        assert_eq!(
            neg.checked_abs().map(|x| x.to_fixed().to_raw()),
            Some(1_500_000)
        );
        let pos = UsdcAmount::from_raw(1_500_000);
        assert_eq!(
            pos.checked_abs().map(|x| x.to_fixed().to_raw()),
            Some(1_500_000)
        );
    }

    #[test]
    fn assign_ops() {
        let mut a = UsdcAmount::from_raw(1_000_000);
        a += UsdcAmount::from_raw(500_000);
        assert_eq!(a.to_fixed().to_raw(), 1_500_000);
        a -= UsdcAmount::from_raw(250_000);
        assert_eq!(a.to_fixed().to_raw(), 1_250_000);
    }

    #[test]
    fn from_fixed_roundtrip() {
        let f = FixedI64::<6>::from_raw(42_000_000);
        let u = UsdcAmount::from_fixed(f);
        assert_eq!(u.to_fixed(), f);
    }
}
