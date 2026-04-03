//! Core `FixedI64<D>` type definition.

use mantis_platform::pow10_i64;

/// Fixed-point decimal backed by `i64` with compile-time scale.
///
/// The inner value is scaled by `10^D`. For example, `FixedI64<6>` stores
/// the value 1.5 as `1_500_000`.
///
/// ## Validated scales
///
/// D=2, 4, 6, 8 are tested, benchmarked, and documented as production-ready.
/// Other values up to D=18 compile and work but are not part of the validated set.
///
/// ## Scale guidance
///
/// | D | Use case | Max whole value |
/// |---|----------|-----------------|
/// | 2 | Cents, bps, Polymarket display | ~92 quadrillion |
/// | 4 | Sub-cent precision, some CeFi grids | ~922 trillion |
/// | 6 | USDC, stablecoin math | ~9.2 trillion |
/// | 8 | BTC quantities (satoshi-scale) | ~92 billion |
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct FixedI64<const D: u8>(i64);

impl<const D: u8> FixedI64<D> {
    /// Compile-time assertion that D <= 18 (`POW10_I64` table bound).
    const _BOUND_CHECK: () = assert!(D <= 18, "FixedI64 decimal places D must be <= 18");

    /// The scale factor: `10^D`.
    pub const SCALE: i64 = {
        let () = Self::_BOUND_CHECK;
        pow10_i64(D)
    };

    /// Zero value.
    pub const ZERO: Self = Self(0);

    /// The value `1.0` in this scale (equal to `SCALE`).
    pub const ONE: Self = Self(Self::SCALE);

    /// Smallest representable value (most negative).
    /// For D=6, this is approximately `-9_223_372_036_854.775808`.
    pub const MIN: Self = Self(i64::MIN);

    /// Largest representable value.
    /// For D=6, this is approximately `9_223_372_036_854.775807`.
    pub const MAX: Self = Self(i64::MAX);

    /// Construct from a pre-scaled raw integer.
    /// The caller asserts the value is already in scaled representation.
    #[must_use]
    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    /// Extract the raw scaled integer.
    #[must_use]
    pub const fn to_raw(self) -> i64 {
        self.0
    }

    /// Construct from a whole integer.
    /// Returns `None` if the scaled value overflows `i64`.
    ///
    /// # Examples
    ///
    /// `from_int(42)` at D=6 produces raw `42_000_000`.
    #[must_use]
    pub fn from_int(val: i64) -> Option<Self> {
        val.checked_mul(Self::SCALE).map(Self)
    }

    /// True if the value is zero.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// True if the value is negative (strictly less than zero).
    #[must_use]
    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    /// True if the value is positive (strictly greater than zero).
    #[must_use]
    pub const fn is_positive(self) -> bool {
        self.0 > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_raw_preserves_value() {
        let f: FixedI64<6> = FixedI64::from_raw(1_500_000);
        assert_eq!(f.to_raw(), 1_500_000);
    }

    #[test]
    fn from_int_scales_correctly() {
        let f: FixedI64<6> = FixedI64::from_int(42).expect("should not overflow");
        assert_eq!(f.to_raw(), 42_000_000);
    }

    #[test]
    fn from_int_zero() {
        let f: FixedI64<6> = FixedI64::from_int(0).expect("zero should work");
        assert_eq!(f.to_raw(), 0);
    }

    #[test]
    fn from_int_negative() {
        let f: FixedI64<6> = FixedI64::from_int(-1).expect("negative should work");
        assert_eq!(f.to_raw(), -1_000_000);
    }

    #[test]
    fn from_int_overflow_returns_none() {
        let result: Option<FixedI64<6>> = FixedI64::from_int(i64::MAX);
        assert!(result.is_none());
    }

    #[test]
    fn scale_constants() {
        assert_eq!(FixedI64::<2>::SCALE, 100);
        assert_eq!(FixedI64::<4>::SCALE, 10_000);
        assert_eq!(FixedI64::<6>::SCALE, 1_000_000);
        assert_eq!(FixedI64::<8>::SCALE, 100_000_000);
    }

    #[test]
    fn one_equals_scale() {
        assert_eq!(FixedI64::<6>::ONE.to_raw(), 1_000_000);
        assert_eq!(FixedI64::<8>::ONE.to_raw(), 100_000_000);
    }

    #[test]
    fn zero_is_zero() {
        assert_eq!(FixedI64::<6>::ZERO.to_raw(), 0);
    }

    #[test]
    fn min_max_are_i64_extrema() {
        assert_eq!(FixedI64::<6>::MIN.to_raw(), i64::MIN);
        assert_eq!(FixedI64::<6>::MAX.to_raw(), i64::MAX);
    }

    #[test]
    fn is_zero_true_for_zero() {
        assert!(FixedI64::<6>::ZERO.is_zero());
    }

    #[test]
    fn is_zero_false_for_nonzero() {
        assert!(!FixedI64::<6>::ONE.is_zero());
    }

    #[test]
    fn is_negative_and_positive() {
        let pos = FixedI64::<6>::from_raw(1);
        let neg = FixedI64::<6>::from_raw(-1);
        let zero = FixedI64::<6>::ZERO;

        assert!(pos.is_positive());
        assert!(!pos.is_negative());

        assert!(neg.is_negative());
        assert!(!neg.is_positive());

        assert!(!zero.is_positive());
        assert!(!zero.is_negative());
    }

    #[test]
    fn ordering_works() {
        let a = FixedI64::<6>::from_raw(100);
        let b = FixedI64::<6>::from_raw(200);
        let c = FixedI64::<6>::from_raw(-50);
        assert!(a < b);
        assert!(c < a);
        assert!(c < b);
    }
}
