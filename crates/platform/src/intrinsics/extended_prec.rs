//! Extended precision arithmetic: widening multiply and multi-limb accumulation.
//!
//! Maps to Constantine's `extended_precision.nim` and
//! `extended_precision_64bit_uint128.nim`.
//!
//! All operations are branchless and suitable for constant-time use.

use crate::constant_time::{Carry, Ct};

use super::addcarry_subborrow::AddCarryOp;

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Widening multiply: `(hi, lo) <- self * rhs`.
///
/// Returns `(hi, lo)` where the concatenation `hi:lo` holds the full product.
pub trait WideMul: Sized {
    /// Computes `(hi, lo) <- self * rhs` using widening arithmetic.
    #[must_use]
    fn wide_mul(self, rhs: Self) -> (Self, Self);
}

/// Widening multiply + single add: `(hi, lo) <- self * rhs + c`.
///
/// Adding one maximum limb to the full-width product cannot overflow.
pub trait WideMulAdd1: WideMul {
    /// Computes `(hi, lo) <- self * rhs + c` using widening arithmetic.
    #[must_use]
    fn muladd1(self, rhs: Self, c: Self) -> (Self, Self);
}

/// Widening multiply + double add: `(hi, lo) <- self * rhs + c1 + c2`.
///
/// Adding two maximum limbs to the full-width product cannot overflow.
pub trait WideMulAdd2: WideMul {
    /// Computes `(hi, lo) <- self * rhs + c1 + c2` using widening arithmetic.
    #[must_use]
    fn muladd2(self, rhs: Self, c1: Self, c2: Self) -> (Self, Self);
}

/// Signed widening multiply: `(hi, lo) <- self * rhs` (signed interpretation).
///
/// Reinterprets the unsigned bits as signed, performs a signed widening
/// multiply, and returns the result bits as unsigned.
pub trait SignedWideMul: Sized {
    /// Computes `(hi, lo) <- self * rhs` with signed widening arithmetic.
    ///
    /// Bits are reinterpreted as signed before multiplying and reinterpreted
    /// back as unsigned on return.
    #[must_use]
    fn smul(self, rhs: Self) -> (Self, Self);
}

// ---------------------------------------------------------------------------
// Ct<u32> impls — widen to u64
// ---------------------------------------------------------------------------

impl WideMul for Ct<u32> {
    #[inline]
    fn wide_mul(self, rhs: Self) -> (Self, Self) {
        let wide = u64::from(self.inner()) * u64::from(rhs.inner());
        // Intentional truncation: low 32 bits are `lo`, high 32 bits are `hi`.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "intentional widening-split pattern"
        )]
        let lo = Ct::new(wide as u32);
        let hi = Ct::new((wide >> 32) as u32);
        (hi, lo)
    }
}

impl WideMulAdd1 for Ct<u32> {
    #[inline]
    fn muladd1(self, rhs: Self, c: Self) -> (Self, Self) {
        // max² + max = (0xFFFFFFFE_00000001) + 0xFFFFFFFF = 0xFFFFFFFF_00000000 — fits in u64.
        let wide = u64::from(self.inner()) * u64::from(rhs.inner()) + u64::from(c.inner());
        #[expect(
            clippy::cast_possible_truncation,
            reason = "intentional widening-split pattern"
        )]
        let lo = Ct::new(wide as u32);
        let hi = Ct::new((wide >> 32) as u32);
        (hi, lo)
    }
}

impl WideMulAdd2 for Ct<u32> {
    #[inline]
    fn muladd2(self, rhs: Self, c1: Self, c2: Self) -> (Self, Self) {
        // max² + max + max = 0xFFFFFFFF_00000000 + 0xFFFFFFFF = fits in u64.
        let wide = u64::from(self.inner()) * u64::from(rhs.inner())
            + u64::from(c1.inner())
            + u64::from(c2.inner());
        #[expect(
            clippy::cast_possible_truncation,
            reason = "intentional widening-split pattern"
        )]
        let lo = Ct::new(wide as u32);
        let hi = Ct::new((wide >> 32) as u32);
        (hi, lo)
    }
}

impl SignedWideMul for Ct<u32> {
    #[inline]
    fn smul(self, rhs: Self) -> (Self, Self) {
        // Reinterpret bits as i32, widen to i64, multiply, reinterpret back.
        // The `as i32` cast is intentional bit-reinterpretation (same bit pattern).
        #[expect(
            clippy::cast_possible_wrap,
            reason = "intentional bit-reinterpretation: u32 -> i32"
        )]
        let wide = i64::from(self.inner() as i32) * i64::from(rhs.inner() as i32);
        // Reinterpret result bits back as unsigned — sign-extended high word is correct.
        #[expect(
            clippy::cast_sign_loss,
            reason = "intentional bit-reinterpretation: i32 -> u32"
        )]
        #[expect(
            clippy::cast_possible_truncation,
            reason = "intentional widening-split pattern"
        )]
        let lo = Ct::new(wide as u32);
        // After shifting right 32 bits the i64 value fits in i32, so the only lint is sign loss.
        #[expect(
            clippy::cast_sign_loss,
            reason = "intentional bit-reinterpretation: i32 -> u32"
        )]
        let hi = Ct::new((wide >> 32) as u32);
        (hi, lo)
    }
}

// ---------------------------------------------------------------------------
// Ct<u64> impls — widen to u128
// ---------------------------------------------------------------------------

impl WideMul for Ct<u64> {
    #[inline]
    fn wide_mul(self, rhs: Self) -> (Self, Self) {
        let wide = u128::from(self.inner()) * u128::from(rhs.inner());
        // Intentional truncation: low 64 bits are `lo`, high 64 bits are `hi`.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "intentional widening-split pattern"
        )]
        let lo = Ct::new(wide as u64);
        let hi = Ct::new((wide >> 64) as u64);
        (hi, lo)
    }
}

impl WideMulAdd1 for Ct<u64> {
    #[inline]
    fn muladd1(self, rhs: Self, c: Self) -> (Self, Self) {
        // max² + max fits in u128 — no overflow.
        let wide = u128::from(self.inner()) * u128::from(rhs.inner()) + u128::from(c.inner());
        #[expect(
            clippy::cast_possible_truncation,
            reason = "intentional widening-split pattern"
        )]
        let lo = Ct::new(wide as u64);
        let hi = Ct::new((wide >> 64) as u64);
        (hi, lo)
    }
}

impl WideMulAdd2 for Ct<u64> {
    #[inline]
    fn muladd2(self, rhs: Self, c1: Self, c2: Self) -> (Self, Self) {
        // max² + max + max fits in u128 — no overflow.
        let wide = u128::from(self.inner()) * u128::from(rhs.inner())
            + u128::from(c1.inner())
            + u128::from(c2.inner());
        #[expect(
            clippy::cast_possible_truncation,
            reason = "intentional widening-split pattern"
        )]
        let lo = Ct::new(wide as u64);
        let hi = Ct::new((wide >> 64) as u64);
        (hi, lo)
    }
}

impl SignedWideMul for Ct<u64> {
    #[inline]
    fn smul(self, rhs: Self) -> (Self, Self) {
        // Reinterpret bits as i64, widen to i128, multiply, reinterpret back.
        // The `as i64` cast is intentional bit-reinterpretation (same bit pattern).
        #[expect(
            clippy::cast_possible_wrap,
            reason = "intentional bit-reinterpretation: u64 -> i64"
        )]
        let wide = i128::from(self.inner() as i64) * i128::from(rhs.inner() as i64);
        #[expect(
            clippy::cast_sign_loss,
            reason = "intentional bit-reinterpretation: i64 -> u64"
        )]
        #[expect(
            clippy::cast_possible_truncation,
            reason = "intentional widening-split pattern"
        )]
        let lo = Ct::new(wide as u64);
        // After shifting right 64 bits the i128 value fits in i64, so only sign loss fires.
        #[expect(
            clippy::cast_sign_loss,
            reason = "intentional bit-reinterpretation: i64 -> u64"
        )]
        let hi = Ct::new((wide >> 64) as u64);
        (hi, lo)
    }
}

// ---------------------------------------------------------------------------
// Free functions — 3-limb accumulation (t, u, v) += a * b
// ---------------------------------------------------------------------------

/// 3-limb accumulate for `u64` limbs: `(t, u, v) += a * b`.
///
/// `(t, u, v)` is a 3-limb little-endian accumulator.  The product is added
/// into `v` (low), carry propagates into `u` (mid) and then `t` (high).
#[inline]
#[expect(
    clippy::many_single_char_names,
    reason = "canonical multi-limb accumulator notation from Constantine"
)]
pub fn mul_acc(t: &mut Ct<u64>, u: &mut Ct<u64>, v: &mut Ct<u64>, a: Ct<u64>, b: Ct<u64>) {
    let (uv_hi, uv_lo) = a.wide_mul(b);
    let (new_v, carry) = (*v).add_c(uv_lo, Carry::new(0));
    *v = new_v;
    let (new_u, carry) = (*u).add_c(uv_hi, carry);
    *u = new_u;
    *t += Ct::new(u64::from(carry.inner()));
}

/// 3-limb accumulate for `u32` limbs: `(t, u, v) += a * b`.
#[inline]
#[expect(
    clippy::many_single_char_names,
    reason = "canonical multi-limb accumulator notation from Constantine"
)]
pub fn mul_acc32(t: &mut Ct<u32>, u: &mut Ct<u32>, v: &mut Ct<u32>, a: Ct<u32>, b: Ct<u32>) {
    let (uv_hi, uv_lo) = a.wide_mul(b);
    let (new_v, carry) = (*v).add_c(uv_lo, Carry::new(0));
    *v = new_v;
    let (new_u, carry) = (*u).add_c(uv_hi, carry);
    *u = new_u;
    *t += Ct::new(u32::from(carry.inner()));
}

/// 3-limb double-accumulate for `u64` limbs: `(t, u, v) += 2 * a * b`.
///
/// Equivalent to calling `mul_acc` twice with the same `(a, b)`, but computed
/// without an extra multiply by doubling the intermediate product.
#[inline]
#[expect(
    clippy::many_single_char_names,
    reason = "canonical multi-limb accumulator notation from Constantine"
)]
pub fn mul_double_acc(t: &mut Ct<u64>, u: &mut Ct<u64>, v: &mut Ct<u64>, a: Ct<u64>, b: Ct<u64>) {
    let (uv_hi, uv_lo) = a.wide_mul(b);
    // Double the product: UV = UV + UV (i.e. shift left by 1).
    let (uv_lo_d, carry_lo) = uv_lo.add_c(uv_lo, Carry::new(0));
    let (uv_hi_d, carry_hi) = uv_hi.add_c(uv_hi, carry_lo);
    // Both carry_hi (from doubling) and carry (from accumulation) are at most 1.
    // Their sum ≤ 2, so t only overflows if the 3-limb accumulator was already saturated.
    *t += Ct::new(u64::from(carry_hi.inner()));
    // Accumulate the doubled product into (t, u, v).
    let (new_v, carry) = (*v).add_c(uv_lo_d, Carry::new(0));
    *v = new_v;
    let (new_u, carry) = (*u).add_c(uv_hi_d, carry);
    *u = new_u;
    *t += Ct::new(u64::from(carry.inner()));
}

/// 3-limb double-accumulate for `u32` limbs: `(t, u, v) += 2 * a * b`.
#[inline]
#[expect(
    clippy::many_single_char_names,
    reason = "canonical multi-limb accumulator notation from Constantine"
)]
pub fn mul_double_acc32(t: &mut Ct<u32>, u: &mut Ct<u32>, v: &mut Ct<u32>, a: Ct<u32>, b: Ct<u32>) {
    let (uv_hi, uv_lo) = a.wide_mul(b);
    let (uv_lo_d, carry_lo) = uv_lo.add_c(uv_lo, Carry::new(0));
    let (uv_hi_d, carry_hi) = uv_hi.add_c(uv_hi, carry_lo);
    // Both carry_hi (from doubling) and carry (from accumulation) are at most 1.
    // Their sum ≤ 2, so t only overflows if the 3-limb accumulator was already saturated.
    *t += Ct::new(u32::from(carry_hi.inner()));
    let (new_v, carry) = (*v).add_c(uv_lo_d, Carry::new(0));
    *v = new_v;
    let (new_u, carry) = (*u).add_c(uv_hi_d, carry);
    *u = new_u;
    *t += Ct::new(u32::from(carry.inner()));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- WideMul Ct<u32> ---

    #[test]
    fn wide_mul_u32_zero() {
        let (hi, lo) = Ct::new(0u32).wide_mul(Ct::new(0u32));
        assert_eq!(hi.inner(), 0u32);
        assert_eq!(lo.inner(), 0u32);
    }

    #[test]
    fn wide_mul_u32_one() {
        let (hi, lo) = Ct::new(1u32).wide_mul(Ct::new(1u32));
        assert_eq!(hi.inner(), 0u32);
        assert_eq!(lo.inner(), 1u32);
    }

    #[test]
    fn wide_mul_u32_small() {
        // 3 * 5 = 15
        let (hi, lo) = Ct::new(3u32).wide_mul(Ct::new(5u32));
        assert_eq!(hi.inner(), 0u32);
        assert_eq!(lo.inner(), 15u32);
    }

    #[test]
    fn wide_mul_u32_max_times_max() {
        // 0xFFFFFFFF * 0xFFFFFFFF = 0xFFFFFFFE_00000001
        let (hi, lo) = Ct::new(u32::MAX).wide_mul(Ct::new(u32::MAX));
        let expected = u64::from(u32::MAX) * u64::from(u32::MAX);
        assert_eq!(
            u64::from(hi.inner()) << 32 | u64::from(lo.inner()),
            expected
        );
        assert_eq!(hi.inner(), 0xFFFF_FFFEu32);
        assert_eq!(lo.inner(), 0x0000_0001u32);
    }

    // --- WideMul Ct<u64> ---

    #[test]
    fn wide_mul_u64_zero() {
        let (hi, lo) = Ct::new(0u64).wide_mul(Ct::new(0u64));
        assert_eq!(hi.inner(), 0u64);
        assert_eq!(lo.inner(), 0u64);
    }

    #[test]
    fn wide_mul_u64_one() {
        let (hi, lo) = Ct::new(1u64).wide_mul(Ct::new(1u64));
        assert_eq!(hi.inner(), 0u64);
        assert_eq!(lo.inner(), 1u64);
    }

    #[test]
    fn wide_mul_u64_small() {
        let (hi, lo) = Ct::new(7u64).wide_mul(Ct::new(8u64));
        assert_eq!(hi.inner(), 0u64);
        assert_eq!(lo.inner(), 56u64);
    }

    #[test]
    fn wide_mul_u64_max_times_max() {
        // 0xFFFFFFFF_FFFFFFFF² = 0xFFFFFFFFFFFFFFFE_0000000000000001
        let (hi, lo) = Ct::new(u64::MAX).wide_mul(Ct::new(u64::MAX));
        let expected = u128::from(u64::MAX) * u128::from(u64::MAX);
        assert_eq!(
            u128::from(hi.inner()) << 64 | u128::from(lo.inner()),
            expected
        );
        assert_eq!(hi.inner(), 0xFFFF_FFFF_FFFF_FFFEu64);
        assert_eq!(lo.inner(), 0x0000_0000_0000_0001u64);
    }

    // --- WideMulAdd1 ---

    #[test]
    fn muladd1_u32_basic() {
        // 2 * 3 + 4 = 10
        let (hi, lo) = Ct::new(2u32).muladd1(Ct::new(3u32), Ct::new(4u32));
        assert_eq!(hi.inner(), 0u32);
        assert_eq!(lo.inner(), 10u32);
    }

    #[test]
    fn muladd1_u32_max_squared_plus_max() {
        // 0xFFFFFFFF² + 0xFFFFFFFF = 0xFFFFFFFF_00000000 — no overflow
        let (hi, lo) = Ct::new(u32::MAX).muladd1(Ct::new(u32::MAX), Ct::new(u32::MAX));
        let expected = u64::from(u32::MAX) * u64::from(u32::MAX) + u64::from(u32::MAX);
        assert_eq!(
            u64::from(hi.inner()) << 32 | u64::from(lo.inner()),
            expected
        );
    }

    #[test]
    fn muladd1_u64_max_squared_plus_max() {
        let (hi, lo) = Ct::new(u64::MAX).muladd1(Ct::new(u64::MAX), Ct::new(u64::MAX));
        let expected = u128::from(u64::MAX) * u128::from(u64::MAX) + u128::from(u64::MAX);
        assert_eq!(
            u128::from(hi.inner()) << 64 | u128::from(lo.inner()),
            expected
        );
    }

    // --- WideMulAdd2 ---

    #[test]
    fn muladd2_u32_basic() {
        // 2 * 3 + 1 + 2 = 9
        let (hi, lo) = Ct::new(2u32).muladd2(Ct::new(3u32), Ct::new(1u32), Ct::new(2u32));
        assert_eq!(hi.inner(), 0u32);
        assert_eq!(lo.inner(), 9u32);
    }

    #[test]
    fn muladd2_u32_max_squared_plus_two_max() {
        let (hi, lo) =
            Ct::new(u32::MAX).muladd2(Ct::new(u32::MAX), Ct::new(u32::MAX), Ct::new(u32::MAX));
        let expected =
            u64::from(u32::MAX) * u64::from(u32::MAX) + u64::from(u32::MAX) + u64::from(u32::MAX);
        assert_eq!(
            u64::from(hi.inner()) << 32 | u64::from(lo.inner()),
            expected
        );
    }

    #[test]
    fn muladd2_u64_max_squared_plus_two_max() {
        let (hi, lo) =
            Ct::new(u64::MAX).muladd2(Ct::new(u64::MAX), Ct::new(u64::MAX), Ct::new(u64::MAX));
        let expected = u128::from(u64::MAX) * u128::from(u64::MAX)
            + u128::from(u64::MAX)
            + u128::from(u64::MAX);
        assert_eq!(
            u128::from(hi.inner()) << 64 | u128::from(lo.inner()),
            expected
        );
    }

    // --- SignedWideMul Ct<u32> ---

    #[test]
    fn smul_u32_positive_positive() {
        // 3 * 5 = 15
        let (hi, lo) = Ct::new(3u32).smul(Ct::new(5u32));
        assert_eq!(hi.inner(), 0u32);
        assert_eq!(lo.inner(), 15u32);
    }

    #[test]
    fn smul_u32_negative_negative() {
        // (-1) * (-1) = 1 (bit pattern: 0xFFFFFFFF * 0xFFFFFFFF signed)
        let (hi, lo) = Ct::new(u32::MAX).smul(Ct::new(u32::MAX));
        // i64: (-1) * (-1) = 1
        assert_eq!(hi.inner(), 0u32);
        assert_eq!(lo.inner(), 1u32);
    }

    #[test]
    fn smul_u32_positive_negative() {
        // 1 * (-1) = -1 => hi = 0xFFFFFFFF, lo = 0xFFFFFFFF
        let (hi, lo) = Ct::new(1u32).smul(Ct::new(u32::MAX));
        // i64: 1 * (-1) = -1 = 0xFFFFFFFF_FFFFFFFF
        assert_eq!(hi.inner(), 0xFFFF_FFFFu32);
        assert_eq!(lo.inner(), 0xFFFF_FFFFu32);
    }

    #[test]
    fn smul_u32_zero() {
        let (hi, lo) = Ct::new(0u32).smul(Ct::new(u32::MAX));
        assert_eq!(hi.inner(), 0u32);
        assert_eq!(lo.inner(), 0u32);
    }

    // --- SignedWideMul Ct<u64> ---

    #[test]
    fn smul_u64_positive_positive() {
        let (hi, lo) = Ct::new(4u64).smul(Ct::new(5u64));
        assert_eq!(hi.inner(), 0u64);
        assert_eq!(lo.inner(), 20u64);
    }

    #[test]
    fn smul_u64_negative_negative() {
        // (-1) * (-1) = 1
        let (hi, lo) = Ct::new(u64::MAX).smul(Ct::new(u64::MAX));
        assert_eq!(hi.inner(), 0u64);
        assert_eq!(lo.inner(), 1u64);
    }

    #[test]
    fn smul_u64_mixed_sign() {
        // 1 * (-1) = -1 => hi = 0xFFFFFFFF_FFFFFFFF, lo = 0xFFFFFFFF_FFFFFFFF
        let (hi, lo) = Ct::new(1u64).smul(Ct::new(u64::MAX));
        assert_eq!(hi.inner(), u64::MAX);
        assert_eq!(lo.inner(), u64::MAX);
    }

    #[test]
    fn smul_u64_zero() {
        let (hi, lo) = Ct::new(0u64).smul(Ct::new(u64::MAX));
        assert_eq!(hi.inner(), 0u64);
        assert_eq!(lo.inner(), 0u64);
    }

    // --- mul_acc / mul_acc32 ---

    #[test]
    fn mul_acc_from_zero() {
        let mut t = Ct::new(0u64);
        let mut u = Ct::new(0u64);
        let mut v = Ct::new(0u64);
        mul_acc(&mut t, &mut u, &mut v, Ct::new(3u64), Ct::new(5u64));
        // 3 * 5 = 15; no carry
        assert_eq!(t.inner(), 0u64);
        assert_eq!(u.inner(), 0u64);
        assert_eq!(v.inner(), 15u64);
    }

    #[test]
    fn mul_acc32_from_zero() {
        let mut t = Ct::new(0u32);
        let mut u = Ct::new(0u32);
        let mut v = Ct::new(0u32);
        mul_acc32(&mut t, &mut u, &mut v, Ct::new(3u32), Ct::new(5u32));
        assert_eq!(t.inner(), 0u32);
        assert_eq!(u.inner(), 0u32);
        assert_eq!(v.inner(), 15u32);
    }

    #[test]
    fn mul_acc_into_nonzero() {
        // Accumulate into (0, 0, 10): 10 + 3*5 = 25
        let mut t = Ct::new(0u64);
        let mut u = Ct::new(0u64);
        let mut v = Ct::new(10u64);
        mul_acc(&mut t, &mut u, &mut v, Ct::new(3u64), Ct::new(5u64));
        assert_eq!(t.inner(), 0u64);
        assert_eq!(u.inner(), 0u64);
        assert_eq!(v.inner(), 25u64);
    }

    #[test]
    fn mul_acc_carry_into_u() {
        // max * max = (0xFFFFFFFFFFFFFFFE, 0x0000000000000001)
        // Add that into (0, 0, 0): v=1, u=0xFFFFFFFFFFFFFFFE, t=0
        let mut t = Ct::new(0u64);
        let mut u = Ct::new(0u64);
        let mut v = Ct::new(0u64);
        mul_acc(&mut t, &mut u, &mut v, Ct::new(u64::MAX), Ct::new(u64::MAX));
        assert_eq!(t.inner(), 0u64);
        assert_eq!(u.inner(), 0xFFFF_FFFF_FFFF_FFFEu64);
        assert_eq!(v.inner(), 1u64);
    }

    #[test]
    #[expect(
        clippy::many_single_char_names,
        reason = "matches accumulator API signature"
    )]
    fn mul_acc_carry_into_t() {
        let mut t = Ct::new(0u64);
        let mut u = Ct::new(u64::MAX);
        let mut v = Ct::new(0u64);
        let a = Ct::new(u64::MAX);
        let b = Ct::new(u64::MAX);
        mul_acc(&mut t, &mut u, &mut v, a, b);
        // a*b = (0xFFFFFFFFFFFFFFFE, 0x0000000000000001)
        // v = 0 + 1 = 1, carry=0
        // u = MAX + 0xFFFFFFFFFFFFFFFE + 0 = MAX-1 (wraps), carry=1
        // t = 0 + 1 = 1
        assert_eq!(v.inner(), 1u64);
        assert_eq!(t.inner(), 1u64);
    }

    // --- mul_double_acc / mul_double_acc32 ---

    #[test]
    fn mul_double_acc_equals_two_mul_acc() {
        // mul_double_acc(t,u,v, a, b) should equal calling mul_acc twice.
        let a = Ct::new(0x1234_5678_9ABC_DEF0u64);
        let b = Ct::new(0xFEDC_BA98_7654_3210u64);

        let mut t1 = Ct::new(0u64);
        let mut u1 = Ct::new(0u64);
        let mut v1 = Ct::new(0u64);
        mul_acc(&mut t1, &mut u1, &mut v1, a, b);
        mul_acc(&mut t1, &mut u1, &mut v1, a, b);

        let mut t2 = Ct::new(0u64);
        let mut u2 = Ct::new(0u64);
        let mut v2 = Ct::new(0u64);
        mul_double_acc(&mut t2, &mut u2, &mut v2, a, b);

        assert_eq!(t1.inner(), t2.inner(), "t mismatch");
        assert_eq!(u1.inner(), u2.inner(), "u mismatch");
        assert_eq!(v1.inner(), v2.inner(), "v mismatch");
    }

    #[test]
    fn mul_double_acc32_equals_two_mul_acc32() {
        let a = Ct::new(0x1234_5678u32);
        let b = Ct::new(0xFEDC_BA98u32);

        let mut t1 = Ct::new(0u32);
        let mut u1 = Ct::new(0u32);
        let mut v1 = Ct::new(0u32);
        mul_acc32(&mut t1, &mut u1, &mut v1, a, b);
        mul_acc32(&mut t1, &mut u1, &mut v1, a, b);

        let mut t2 = Ct::new(0u32);
        let mut u2 = Ct::new(0u32);
        let mut v2 = Ct::new(0u32);
        mul_double_acc32(&mut t2, &mut u2, &mut v2, a, b);

        assert_eq!(t1.inner(), t2.inner(), "t mismatch");
        assert_eq!(u1.inner(), u2.inner(), "u mismatch");
        assert_eq!(v1.inner(), v2.inner(), "v mismatch");
    }

    #[test]
    fn mul_double_acc_small() {
        // 2 * (3 * 5) = 30
        let mut t = Ct::new(0u64);
        let mut u = Ct::new(0u64);
        let mut v = Ct::new(0u64);
        mul_double_acc(&mut t, &mut u, &mut v, Ct::new(3u64), Ct::new(5u64));
        assert_eq!(t.inner(), 0u64);
        assert_eq!(u.inner(), 0u64);
        assert_eq!(v.inner(), 30u64);
    }
}
