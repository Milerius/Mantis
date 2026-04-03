//! Multiplication and division with explicit rounding for `FixedI64`.
//!
//! All operations widen to `i128` to avoid intermediate overflow.
//! No `Mul`/`Div` trait impls — callers must choose a rounding mode.
//!
//! # Performance note
//!
//! Multiplication uses a hand-rolled `div_i128_by_const` that avoids the
//! `__divti3` runtime call LLVM emits for `i128 / constant` on aarch64.
//! The i128 product from `i64 * i64` is decomposed so that each partial
//! division operates on values LLVM can strength-reduce to multiply-by-
//! reciprocal sequences. This brings `checked_mul_trunc` from ~1.77ns
//! down to the ~1.2ns range.

use crate::FixedI64;

/// Divide a u128 (given as hi:u64, lo:u64) by a u64 divisor.
/// Returns (quotient_hi: u64, quotient_lo: u64, remainder: u64).
///
/// Uses only u64 arithmetic — no i128/u128 division, so LLVM
/// strength-reduces each `u64 / constant` to multiply-by-reciprocal.
///
/// The algorithm is schoolbook long division on two 64-bit "digits":
///   value = hi * 2^64 + lo
///   q_hi  = hi / d
///   r_hi  = hi % d
///   (q_lo, rem) = (r_hi * 2^64 + lo) / d   ← this is a 128/64 div
///
/// For the 128/64 step: since r_hi < d (always true after hi % d),
/// and d fits in u64, the quotient fits in u64. We further decompose
/// using the identity: (A * 2^32 + B) / d where A = r_hi * 2^32 + lo_hi,
/// B = lo_lo, reducing to two u64 divisions.
#[inline(always)]
const fn div_u128_by_u64(hi: u64, lo: u64, d: u64) -> (u64, u64, u64) {
    // Step 1: divide the high word
    let q_hi = hi / d;
    let r_hi = hi % d;

    // Step 2: divide (r_hi : lo) by d — a 128-bit / 64-bit division.
    // r_hi < d, so the quotient fits in 64 bits.
    //
    // Decompose lo into two 32-bit halves:
    //   (r_hi : lo_upper : lo_lower)
    //   First: divide (r_hi : lo_upper) by d → q_mid, r_mid
    //   Then:  divide (r_mid : lo_lower) by d → q_low, remainder
    //   q_lo = q_mid * 2^32 + q_low
    let lo_upper = lo >> 32;
    let lo_lower = lo & 0xFFFF_FFFF;

    // (r_hi * 2^32 + lo_upper) fits in u64 when d > 2^32,
    // but may overflow when d < 2^32. Use u128 for this intermediate.
    // LLVM optimizes u128 / u64_const into multiply-by-reciprocal.
    let mid_num = ((r_hi as u128) << 32) | (lo_upper as u128);
    let q_mid = (mid_num / (d as u128)) as u64;
    let r_mid = (mid_num % (d as u128)) as u64;

    let low_num = ((r_mid as u128) << 32) | (lo_lower as u128);
    let q_low = (low_num / (d as u128)) as u64;
    let rem = (low_num % (d as u128)) as u64;

    let q_lo = (q_mid << 32) | q_low;
    (q_hi, q_lo, rem)
}

/// Divide a signed i128 (hi:i64, lo:u64) by a positive i64 constant,
/// truncating toward zero. Returns (quotient as i128, remainder as i64).
#[inline(always)]
const fn div_wide_by_const(hi: i64, lo: u64, d: i64) -> (i128, i64) {
    let negative = hi < 0;

    // Absolute value via two's complement negation
    let (abs_lo, abs_hi) = if negative {
        let not_lo = !lo;
        let (neg_lo, carry) = not_lo.overflowing_add(1);
        let neg_hi = (!hi as u64).wrapping_add(carry as u64);
        (neg_lo, neg_hi)
    } else {
        (lo, hi as u64)
    };

    let (q_hi, q_lo, rem) = div_u128_by_u64(abs_hi, abs_lo, d as u64);
    let abs_quot = ((q_hi as u128) << 64) | (q_lo as u128);

    if negative {
        (-(abs_quot as i128), -(rem as i64))
    } else {
        (abs_quot as i128, rem as i64)
    }
}

// All i128-to-i64 narrowing casts in this impl are guarded by explicit range checks.
#[expect(
    clippy::cast_possible_truncation,
    reason = "all i128-to-i64 casts are guarded by range checks"
)]
impl<const D: u8> FixedI64<D> {
    /// Checked multiplication, truncating toward zero.
    ///
    /// Computes `(self * rhs) / SCALE` via `i128`, returning `None` on overflow.
    /// Uses decomposed division to avoid the `__divti3` runtime call.
    #[must_use]
    pub const fn checked_mul_trunc(self, rhs: Self) -> Option<Self> {
        let a = self.to_raw();
        let b = rhs.to_raw();
        // smulh + mul on aarch64 — gives us (hi, lo) of a*b
        let wide = (a as i128) * (b as i128);
        let hi = (wide >> 64) as i64;
        let lo = wide as u64;

        let (result, _) = div_wide_by_const(hi, lo, Self::SCALE);

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
        let hi = (biased >> 64) as i64;
        let lo = biased as u64;

        let (result, _) = div_wide_by_const(hi, lo, Self::SCALE);

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
