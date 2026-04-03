//! Multiplication and division with explicit rounding for `FixedI64`.
//!
//! All operations widen to `i128` to avoid intermediate overflow.
//! No `Mul`/`Div` trait impls — callers must choose a rounding mode.

use crate::FixedI64;

// All i128-to-i64 narrowing casts in this impl are guarded by explicit range checks.
#[expect(
    clippy::cast_possible_truncation,
    reason = "all i128-to-i64 casts are guarded by range checks"
)]
impl<const D: u8> FixedI64<D> {
    /// Checked multiplication, truncating toward zero.
    ///
    /// Computes `(self * rhs) / SCALE` via `i128`, returning `None` on overflow.
    #[must_use]
    pub const fn checked_mul_trunc(self, rhs: Self) -> Option<Self> {
        let wide = (self.to_raw() as i128) * (rhs.to_raw() as i128);
        let result = wide / (Self::SCALE as i128);
        if result > (i64::MAX as i128) || result < (i64::MIN as i128) {
            None
        } else {
            Some(Self::from_raw(result as i64))
        }
    }

    /// Checked multiplication, rounding ties away from zero.
    ///
    /// Biases by `+/- SCALE/2` (sign-aware) before the division.
    #[must_use]
    pub const fn checked_mul_round(self, rhs: Self) -> Option<Self> {
        let wide = (self.to_raw() as i128) * (rhs.to_raw() as i128);
        let scale = Self::SCALE as i128;
        let half = scale / 2;
        let biased = if wide >= 0 { wide + half } else { wide - half };
        let result = biased / scale;
        if result > (i64::MAX as i128) || result < (i64::MIN as i128) {
            None
        } else {
            Some(Self::from_raw(result as i64))
        }
    }

    /// Saturating multiplication, truncating toward zero.
    ///
    /// Clamps to `MAX` or `MIN` on overflow.
    #[must_use]
    pub const fn saturating_mul_trunc(self, rhs: Self) -> Self {
        if let Some(v) = self.checked_mul_trunc(rhs) {
            v
        } else {
            // Sign of product determines saturation direction
            let sign_a = self.to_raw() >= 0;
            let sign_b = rhs.to_raw() >= 0;
            if sign_a == sign_b {
                Self::MAX
            } else {
                Self::MIN
            }
        }
    }

    /// Checked division, truncating toward zero.
    ///
    /// Computes `(self * SCALE) / rhs` via `i128`. Returns `None` on zero divisor
    /// or overflow.
    #[must_use]
    pub const fn checked_div_trunc(self, rhs: Self) -> Option<Self> {
        if rhs.to_raw() == 0 {
            return None;
        }
        let wide = (self.to_raw() as i128) * (Self::SCALE as i128);
        let divisor = rhs.to_raw() as i128;
        let result = wide / divisor;
        if result > (i64::MAX as i128) || result < (i64::MIN as i128) {
            None
        } else {
            Some(Self::from_raw(result as i64))
        }
    }

    /// Checked division, rounding ties away from zero.
    ///
    /// Biases by `+/- abs(divisor)/2` (sign-aware) before dividing.
    /// Returns `None` on zero divisor or overflow.
    #[must_use]
    pub const fn checked_div_round(self, rhs: Self) -> Option<Self> {
        if rhs.to_raw() == 0 {
            return None;
        }
        let wide = (self.to_raw() as i128) * (Self::SCALE as i128);
        let divisor = rhs.to_raw() as i128;
        let abs_div_half = if divisor < 0 {
            (-divisor) / 2
        } else {
            divisor / 2
        };
        let biased = if wide >= 0 {
            wide + abs_div_half
        } else {
            wide - abs_div_half
        };
        let result = biased / divisor;
        if result > (i64::MAX as i128) || result < (i64::MIN as i128) {
            None
        } else {
            Some(Self::from_raw(result as i64))
        }
    }

    /// Checked multiplication by an integer scalar.
    ///
    /// Returns `None` on overflow.
    #[must_use]
    pub const fn checked_mul_int(self, rhs: i64) -> Option<Self> {
        match self.to_raw().checked_mul(rhs) {
            Some(v) => Some(Self::from_raw(v)),
            None => None,
        }
    }

    /// Checked division by an integer scalar, truncating toward zero.
    ///
    /// Returns `None` if `rhs` is zero.
    #[must_use]
    pub const fn checked_div_int(self, rhs: i64) -> Option<Self> {
        if rhs == 0 {
            return None;
        }
        Some(Self::from_raw(self.to_raw() / rhs))
    }

    /// Saturating multiplication by an integer scalar.
    ///
    /// Clamps to `MAX` or `MIN` on overflow.
    #[must_use]
    pub const fn saturating_mul_int(self, rhs: i64) -> Self {
        Self::from_raw(self.to_raw().saturating_mul(rhs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type F2 = FixedI64<2>;
    type F6 = FixedI64<6>;
    type F8 = FixedI64<8>;

    // --- mul_trunc ---

    #[test]
    fn mul_trunc_basic() {
        // 1.50 * 2.00 = 3.00 at D=2
        let a = F2::from_raw(150); // 1.50
        let b = F2::from_raw(200); // 2.00
        assert_eq!(a.checked_mul_trunc(b).map(FixedI64::to_raw), Some(300));
    }

    #[test]
    fn mul_trunc_fractional() {
        // 1.50 * 1.50 = 2.25 at D=2
        let a = F2::from_raw(150);
        let b = F2::from_raw(150);
        assert_eq!(a.checked_mul_trunc(b).map(FixedI64::to_raw), Some(225));
    }

    #[test]
    fn mul_trunc_commutative() {
        let a = F6::from_raw(1_500_000);
        let b = F6::from_raw(3_000_000);
        assert_eq!(a.checked_mul_trunc(b), b.checked_mul_trunc(a));
    }

    #[test]
    fn mul_trunc_by_one() {
        let a = F6::from_raw(1_500_000);
        assert_eq!(a.checked_mul_trunc(F6::ONE), Some(a));
    }

    #[test]
    fn mul_trunc_by_zero() {
        let a = F6::from_raw(1_500_000);
        assert_eq!(a.checked_mul_trunc(F6::ZERO).map(FixedI64::to_raw), Some(0));
    }

    #[test]
    fn mul_trunc_overflow() {
        assert!(F6::MAX.checked_mul_trunc(F6::MAX).is_none());
    }

    #[test]
    fn mul_trunc_truncates_positive() {
        // At D=2: 1.01 * 1.01 = 1.0201 -> truncate to 1.02 (raw 102)
        let a = F2::from_raw(101);
        assert_eq!(a.checked_mul_trunc(a).map(FixedI64::to_raw), Some(102));
    }

    #[test]
    fn mul_trunc_truncates_negative() {
        // -1.01 * 1.01 = -1.0201 -> truncate toward zero = -1.02 (raw -102)
        let a = F2::from_raw(-101);
        let b = F2::from_raw(101);
        assert_eq!(a.checked_mul_trunc(b).map(FixedI64::to_raw), Some(-102));
    }

    // --- mul_round ---

    #[test]
    fn mul_round_positive_tie() {
        // Construct a case where remainder is exactly SCALE/2
        // At D=2 (SCALE=100): a=15, b=15 -> wide=225, half=50
        // 225 + 50 = 275, 275/100 = 2 (no tie here)
        // Need: wide mod SCALE == SCALE/2 -> wide mod 100 == 50
        // a=5 (0.05), b=10 (0.10) -> wide=50. 50+50=100. 100/100=1
        // trunc: 50/100=0. round: 1. Rounds away from zero.
        let a = F2::from_raw(5);
        let b = F2::from_raw(10);
        assert_eq!(a.checked_mul_round(b).map(FixedI64::to_raw), Some(1));
        // Trunc would give 0
        assert_eq!(a.checked_mul_trunc(b).map(FixedI64::to_raw), Some(0));
    }

    #[test]
    fn mul_round_negative_tie() {
        // -0.05 * 0.10 = -0.005 -> tie rounds away from zero = -1 at D=2
        let a = F2::from_raw(-5);
        let b = F2::from_raw(10);
        assert_eq!(a.checked_mul_round(b).map(FixedI64::to_raw), Some(-1));
    }

    #[test]
    fn mul_round_just_below_tie() {
        // wide = 49 at D=2. 49 + 50 = 99. 99/100 = 0. Same as trunc.
        let a = F2::from_raw(7);
        let b = F2::from_raw(7);
        // 7*7=49. trunc: 49/100=0. round: (49+50)/100=0
        assert_eq!(a.checked_mul_round(b).map(FixedI64::to_raw), Some(0));
    }

    #[test]
    fn mul_round_just_above_tie() {
        // wide = 51 at D=2. 51 + 50 = 101. 101/100 = 1. Rounds up.
        // Need wide = 51: e.g. 51 = 3 * 17
        let a = F2::from_raw(3);
        let b = F2::from_raw(17);
        assert_eq!(a.checked_mul_round(b).map(FixedI64::to_raw), Some(1));
    }

    // --- div_trunc ---

    #[test]
    fn div_trunc_basic() {
        // 6.00 / 2.00 = 3.00 at D=2
        let a = F2::from_raw(600);
        let b = F2::from_raw(200);
        assert_eq!(a.checked_div_trunc(b).map(FixedI64::to_raw), Some(300));
    }

    #[test]
    fn div_trunc_by_zero() {
        let a = F2::from_raw(100);
        assert!(a.checked_div_trunc(F2::ZERO).is_none());
    }

    #[test]
    fn div_trunc_min_by_neg_one() {
        // MIN / -1 overflows
        let neg_one = F6::from_int(-1).expect("fits");
        assert!(F6::MIN.checked_div_trunc(neg_one).is_none());
    }

    #[test]
    fn div_trunc_truncates_toward_zero() {
        // 1.00 / 3.00 = 0.33... at D=2 -> truncate to 0.33
        let a = F2::from_raw(100);
        let b = F2::from_raw(300);
        assert_eq!(a.checked_div_trunc(b).map(FixedI64::to_raw), Some(33));
    }

    #[test]
    fn div_trunc_negative_truncates_toward_zero() {
        // -1.00 / 3.00 = -0.33... at D=2 -> truncate to -0.33
        let a = F2::from_raw(-100);
        let b = F2::from_raw(300);
        assert_eq!(a.checked_div_trunc(b).map(FixedI64::to_raw), Some(-33));
    }

    // --- div_round ---

    #[test]
    fn div_round_basic() {
        let a = F2::from_raw(600);
        let b = F2::from_raw(200);
        assert_eq!(a.checked_div_round(b).map(FixedI64::to_raw), Some(300));
    }

    #[test]
    fn div_round_by_zero() {
        assert!(F2::from_raw(100).checked_div_round(F2::ZERO).is_none());
    }

    // --- scalar mul/div ---

    #[test]
    fn checked_mul_int_basic() {
        let a = F6::from_raw(1_500_000); // 1.5
        assert_eq!(a.checked_mul_int(3).map(FixedI64::to_raw), Some(4_500_000));
    }

    #[test]
    fn checked_mul_int_overflow() {
        assert!(F6::MAX.checked_mul_int(2).is_none());
    }

    #[test]
    fn checked_div_int_basic() {
        let a = F6::from_raw(4_500_000);
        assert_eq!(a.checked_div_int(3).map(FixedI64::to_raw), Some(1_500_000));
    }

    #[test]
    fn checked_div_int_by_zero() {
        assert!(F6::from_raw(1_000_000).checked_div_int(0).is_none());
    }

    #[test]
    fn saturating_mul_int_overflow() {
        assert_eq!(F6::MAX.saturating_mul_int(2), F6::MAX);
    }

    #[test]
    fn saturating_mul_int_negative_overflow() {
        assert_eq!(F6::MAX.saturating_mul_int(-2), F6::MIN);
    }

    // --- saturating_mul_trunc ---

    #[test]
    fn saturating_mul_trunc_normal() {
        let a = F2::from_raw(150);
        let b = F2::from_raw(200);
        assert_eq!(a.saturating_mul_trunc(b).to_raw(), 300);
    }

    #[test]
    fn saturating_mul_trunc_overflow_positive() {
        assert_eq!(F6::MAX.saturating_mul_trunc(F6::MAX), F6::MAX);
    }

    #[test]
    fn saturating_mul_trunc_overflow_negative() {
        assert_eq!(F6::MAX.saturating_mul_trunc(F6::MIN), F6::MIN);
    }

    // --- cross-scale validation ---

    #[test]
    fn mul_trunc_d2() {
        // 2.50 * 4.00 = 10.00
        let a = F2::from_raw(250);
        let b = F2::from_raw(400);
        assert_eq!(a.checked_mul_trunc(b).map(FixedI64::to_raw), Some(1000));
    }

    #[test]
    fn mul_trunc_d8() {
        // 1.50000000 * 2.00000000 = 3.00000000
        let a = F8::from_raw(150_000_000);
        let b = F8::from_raw(200_000_000);
        assert_eq!(
            a.checked_mul_trunc(b).map(FixedI64::to_raw),
            Some(300_000_000)
        );
    }
}
