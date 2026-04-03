//! Scale conversion between `FixedI64` types with different decimal places.

use mantis_platform::pow10_i64;

use crate::FixedI64;

// All i128-to-i64 narrowing casts in this impl are guarded by explicit range checks.
#[expect(
    clippy::cast_possible_truncation,
    reason = "all i128-to-i64 casts are guarded by range checks"
)]
impl<const D: u8> FixedI64<D> {
    /// Rescale to a different number of decimal places, truncating toward zero
    /// when narrowing.
    ///
    /// - Widening (D2 > D): multiply by `10^(D2 - D)`, `None` on overflow.
    /// - Narrowing (D2 < D): divide by `10^(D - D2)`, truncating.
    /// - Same scale: identity.
    #[must_use]
    pub const fn rescale_trunc<const D2: u8>(self) -> Option<FixedI64<D2>> {
        // Force the target bound check.
        let _ = FixedI64::<D2>::SCALE;

        if D2 == D {
            return Some(FixedI64::<D2>::from_raw(self.to_raw()));
        }

        let raw = self.to_raw() as i128;

        if D2 > D {
            // Widening: multiply
            let factor = pow10_i64(D2 - D) as i128;
            let result = raw * factor;
            if result > (i64::MAX as i128) || result < (i64::MIN as i128) {
                None
            } else {
                Some(FixedI64::<D2>::from_raw(result as i64))
            }
        } else {
            // Narrowing: divide (truncate toward zero)
            let factor = pow10_i64(D - D2) as i128;
            let result = raw / factor;
            if result > (i64::MAX as i128) || result < (i64::MIN as i128) {
                None
            } else {
                Some(FixedI64::<D2>::from_raw(result as i64))
            }
        }
    }

    /// Rescale to a different number of decimal places, rounding ties away
    /// from zero when narrowing.
    ///
    /// - Widening (D2 > D): multiply by `10^(D2 - D)`, `None` on overflow.
    /// - Narrowing (D2 < D): divide with half-up rounding.
    /// - Same scale: identity.
    #[must_use]
    pub const fn rescale_round<const D2: u8>(self) -> Option<FixedI64<D2>> {
        let _ = FixedI64::<D2>::SCALE;

        if D2 == D {
            return Some(FixedI64::<D2>::from_raw(self.to_raw()));
        }

        let raw = self.to_raw() as i128;

        if D2 > D {
            // Widening: same as trunc (no rounding needed)
            let factor = pow10_i64(D2 - D) as i128;
            let result = raw * factor;
            if result > (i64::MAX as i128) || result < (i64::MIN as i128) {
                None
            } else {
                Some(FixedI64::<D2>::from_raw(result as i64))
            }
        } else {
            // Narrowing: divide with rounding
            let factor = pow10_i64(D - D2) as i128;
            let half = factor / 2;
            let biased = if raw >= 0 { raw + half } else { raw - half };
            let result = biased / factor;
            if result > (i64::MAX as i128) || result < (i64::MIN as i128) {
                None
            } else {
                Some(FixedI64::<D2>::from_raw(result as i64))
            }
        }
    }

    /// Rescale exactly. Returns `None` if narrowing would lose digits.
    ///
    /// For widening, this is identical to `rescale_trunc`. For narrowing,
    /// the remainder must be zero.
    #[must_use]
    pub const fn checked_rescale_exact<const D2: u8>(self) -> Option<FixedI64<D2>> {
        let _ = FixedI64::<D2>::SCALE;

        if D2 == D {
            return Some(FixedI64::<D2>::from_raw(self.to_raw()));
        }

        let raw = self.to_raw() as i128;

        if D2 > D {
            // Widening: always exact
            let factor = pow10_i64(D2 - D) as i128;
            let result = raw * factor;
            if result > (i64::MAX as i128) || result < (i64::MIN as i128) {
                None
            } else {
                Some(FixedI64::<D2>::from_raw(result as i64))
            }
        } else {
            // Narrowing: reject if lossy
            let factor = pow10_i64(D - D2) as i128;
            if raw % factor != 0 {
                return None;
            }
            let result = raw / factor;
            if result > (i64::MAX as i128) || result < (i64::MIN as i128) {
                None
            } else {
                Some(FixedI64::<D2>::from_raw(result as i64))
            }
        }
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests use expect for clarity")]
mod tests {
    use crate::FixedI64;

    type F2 = FixedI64<2>;
    type F6 = FixedI64<6>;
    type F8 = FixedI64<8>;

    // --- rescale_trunc ---

    #[test]
    fn widen_d2_to_d6() {
        let a = F2::from_raw(150); // 1.50
        let b: FixedI64<6> = a.rescale_trunc().expect("should fit");
        assert_eq!(b.to_raw(), 1_500_000);
    }

    #[test]
    fn widen_d2_to_d8() {
        let a = F2::from_raw(150);
        let b: FixedI64<8> = a.rescale_trunc().expect("should fit");
        assert_eq!(b.to_raw(), 150_000_000);
    }

    #[test]
    fn widen_overflow() {
        let a = F2::from_raw(i64::MAX);
        let result: Option<FixedI64<6>> = a.rescale_trunc();
        assert!(result.is_none());
    }

    #[test]
    fn narrow_d6_to_d2_trunc() {
        // 1.234567 at D6 -> truncate to 1.23 at D2
        let a = F6::from_raw(1_234_567);
        let b: FixedI64<2> = a.rescale_trunc().expect("should fit");
        assert_eq!(b.to_raw(), 123);
    }

    #[test]
    fn narrow_d6_to_d2_round() {
        // 1.235000 at D6 -> round to 1.24 at D2 (tie away from zero)
        let a = F6::from_raw(1_235_000);
        let b: FixedI64<2> = a.rescale_round().expect("should fit");
        assert_eq!(b.to_raw(), 124);
    }

    #[test]
    fn narrow_negative_trunc() {
        // -1.239000 at D6 -> truncate to -1.23 at D2
        let a = F6::from_raw(-1_239_000);
        let b: FixedI64<2> = a.rescale_trunc().expect("should fit");
        assert_eq!(b.to_raw(), -123);
    }

    #[test]
    fn narrow_negative_round() {
        // -1.235000 at D6 -> round to -1.24 at D2 (tie away from zero)
        let a = F6::from_raw(-1_235_000);
        let b: FixedI64<2> = a.rescale_round().expect("should fit");
        assert_eq!(b.to_raw(), -124);
    }

    #[test]
    fn same_scale_identity() {
        let a = F6::from_raw(1_500_000);
        let b: FixedI64<6> = a.rescale_trunc().expect("identity");
        assert_eq!(a, b);
    }

    // --- checked_rescale_exact ---

    #[test]
    fn exact_widen_lossless() {
        let a = F2::from_raw(150);
        let b: FixedI64<6> = a.checked_rescale_exact().expect("exact widen");
        assert_eq!(b.to_raw(), 1_500_000);
    }

    #[test]
    fn exact_narrow_lossless() {
        let a = F6::from_raw(1_500_000); // 1.500000 -> 1.50 at D2
        let b: FixedI64<2> = a.checked_rescale_exact().expect("exact narrow");
        assert_eq!(b.to_raw(), 150);
    }

    #[test]
    fn exact_narrow_rejects_lossy() {
        let a = F6::from_raw(1_234_567); // has sub-cent digits
        let result: Option<FixedI64<2>> = a.checked_rescale_exact();
        assert!(result.is_none());
    }

    // --- cross-scale ---

    #[test]
    fn narrow_d8_to_d2() {
        let a = F8::from_raw(123_456_789); // 1.23456789
        let b: FixedI64<2> = a.rescale_trunc().expect("fits");
        assert_eq!(b.to_raw(), 123); // 1.23
    }
}
