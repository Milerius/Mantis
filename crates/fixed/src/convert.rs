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

    // --- rescale_trunc: additional mutation killers ---

    #[test]
    fn trunc_vs_round_differ_below_halfway() {
        // 1.234000 at D6 -> D2: factor=10000, remainder=4000 < 5000
        // trunc: 123, round: 123 (biased: 1234000+5000=1239000 / 10000 = 123)
        let a = F6::from_raw(1_234_000);
        let trunc: FixedI64<2> = a.rescale_trunc().expect("fits");
        let round: FixedI64<2> = a.rescale_round().expect("fits");
        assert_eq!(trunc.to_raw(), 123);
        assert_eq!(round.to_raw(), 123);
    }

    #[test]
    fn trunc_vs_round_differ_above_halfway() {
        // 1.236000 at D6 -> D2: factor=10000, remainder=6000 > 5000
        // trunc: 123, round: 124 (biased: 1236000+5000=1241000 / 10000 = 124)
        let a = F6::from_raw(1_236_000);
        let trunc: FixedI64<2> = a.rescale_trunc().expect("fits");
        let round: FixedI64<2> = a.rescale_round().expect("fits");
        assert_eq!(trunc.to_raw(), 123);
        assert_eq!(round.to_raw(), 124);
    }

    #[test]
    fn trunc_and_round_both_produce_exact_on_divisible() {
        // 1.230000 at D6 -> D2: no remainder, both give 123
        let a = F6::from_raw(1_230_000);
        let trunc: FixedI64<2> = a.rescale_trunc().expect("fits");
        let round: FixedI64<2> = a.rescale_round().expect("fits");
        assert_eq!(trunc.to_raw(), 123);
        assert_eq!(round.to_raw(), 123);
    }

    #[test]
    fn trunc_zero_stays_zero() {
        let a = F6::from_raw(0);
        let b: FixedI64<2> = a.rescale_trunc().expect("fits");
        assert_eq!(b.to_raw(), 0);
    }

    #[test]
    fn round_zero_stays_zero() {
        let a = F6::from_raw(0);
        let b: FixedI64<2> = a.rescale_round().expect("fits");
        assert_eq!(b.to_raw(), 0);
    }

    #[test]
    fn widen_negative_d2_to_d6() {
        // -1.50 widened to D6 must produce -1_500_000
        let a = F2::from_raw(-150);
        let b: FixedI64<6> = a.rescale_trunc().expect("fits");
        assert_eq!(b.to_raw(), -1_500_000);
    }

    #[test]
    fn widen_negative_preserves_exact_value() {
        // Widening is always exact, sign must be preserved
        let a = F2::from_raw(-99);
        let b: FixedI64<6> = a.rescale_trunc().expect("fits");
        assert_eq!(b.to_raw(), -990_000);
    }

    #[test]
    fn widen_overflow_negative() {
        // i64::MIN widened should also overflow
        let a = F2::from_raw(i64::MIN);
        let result: Option<FixedI64<6>> = a.rescale_trunc();
        assert!(result.is_none());
    }

    #[test]
    fn same_scale_identity_round() {
        let a = F6::from_raw(-1_500_000);
        let b: FixedI64<6> = a.rescale_round().expect("identity");
        assert_eq!(a, b);
    }

    #[test]
    fn narrow_positive_trunc_never_rounds_up() {
        // 1.999999 at D6 -> D2 truncates to 1.99, never 2.00
        let a = F6::from_raw(1_999_999);
        let b: FixedI64<2> = a.rescale_trunc().expect("fits");
        assert_eq!(b.to_raw(), 199);
    }

    #[test]
    fn narrow_negative_round_away_from_zero_for_tie() {
        // -1.235000 at D6 -> -1.24 at D2 (rounds away from zero)
        // already tested in narrow_negative_round, re-asserting magnitude
        let a = F6::from_raw(-1_235_000);
        let b: FixedI64<2> = a.rescale_round().expect("fits");
        assert_eq!(b.to_raw(), -124); // not -123
    }

    #[test]
    fn narrow_negative_round_below_tie() {
        // -1.234000 at D6 -> -1.23 at D2 (rounds toward zero when below tie)
        // biased: -1234000 - 5000 = -1239000, -1239000/10000 = -123
        let a = F6::from_raw(-1_234_000);
        let b: FixedI64<2> = a.rescale_round().expect("fits");
        assert_eq!(b.to_raw(), -123);
    }

    // --- checked_rescale_exact: additional mutation killers ---

    #[test]
    fn exact_narrow_negative_lossless() {
        // -1.500000 at D6 -> -150 at D2 exactly
        let a = F6::from_raw(-1_500_000);
        let b: FixedI64<2> = a.checked_rescale_exact().expect("exact");
        assert_eq!(b.to_raw(), -150);
    }

    #[test]
    fn exact_narrow_negative_lossy_returns_none() {
        // -1.234567 has sub-cent digits -> None
        let a = F6::from_raw(-1_234_567);
        let result: Option<FixedI64<2>> = a.checked_rescale_exact();
        assert!(result.is_none());
    }

    #[test]
    fn exact_narrow_minimum_lossy() {
        // 1 raw unit at D6 is 0.000001, not representable at D2 -> None
        let a = F6::from_raw(1);
        let result: Option<FixedI64<2>> = a.checked_rescale_exact();
        assert!(result.is_none());
    }

    #[test]
    fn exact_widen_negative() {
        let a = F2::from_raw(-150);
        let b: FixedI64<6> = a.checked_rescale_exact().expect("fits");
        assert_eq!(b.to_raw(), -1_500_000);
    }

    #[test]
    fn exact_widen_overflow_returns_none() {
        let a = F2::from_raw(i64::MAX);
        let result: Option<FixedI64<6>> = a.checked_rescale_exact();
        assert!(result.is_none());
    }

    #[test]
    fn exact_same_scale_identity() {
        let a = F6::from_raw(9_999_999);
        let b: FixedI64<6> = a.checked_rescale_exact().expect("identity");
        assert_eq!(a, b);
    }

    #[test]
    fn exact_narrow_zero() {
        let a = F6::from_raw(0);
        let b: FixedI64<2> = a.checked_rescale_exact().expect("zero is exact");
        assert_eq!(b.to_raw(), 0);
    }

    #[test]
    fn exact_narrow_value_divisible_by_factor() {
        // 10000 raw at D6 = 0.010000 -> 1 raw at D4 exactly
        let a: FixedI64<6> = FixedI64::from_raw(10_000);
        let b: FixedI64<4> = a.checked_rescale_exact().expect("exact");
        assert_eq!(b.to_raw(), 100);
    }

    #[test]
    fn rescale_trunc_factor_not_added_subtracted() {
        // If widening used + instead of *, result would be wrong
        // 1.00 at D2 widened to D6: raw=100, factor=10000, expect 100*10000=1_000_000
        let a = F2::from_raw(100);
        let b: FixedI64<6> = a.rescale_trunc().expect("fits");
        assert_ne!(b.to_raw(), 100 + 10_000); // not addition
        assert_ne!(b.to_raw(), 100 - 10_000); // not subtraction
        assert_eq!(b.to_raw(), 1_000_000);    // must be multiplication
    }

    #[test]
    fn rescale_trunc_narrowing_uses_division_not_subtraction() {
        // If narrowing used - instead of /, result would be wrong
        // 1.230000 at D6 -> D2: raw=1_230_000, factor=10000, expect 1_230_000/10000=123
        let a = F6::from_raw(1_230_000);
        let b: FixedI64<2> = a.rescale_trunc().expect("fits");
        assert_ne!(b.to_raw(), 1_230_000 - 10_000); // not subtraction
        assert_eq!(b.to_raw(), 123);
    }}
