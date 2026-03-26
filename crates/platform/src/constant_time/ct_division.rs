//! Constant-time division of a double-width number by a single-width divisor.
//!
//! Implements the `BearSSL` binary shift algorithm as used in Constantine's
//! `ct_division.nim`. All branching is on public loop indices — no secret
//! values ever reach an `if`/`match` or array index.
//!
//! # Preconditions
//!
//! Both `div2n1n` and `div2n1n_u32` require the inputs to be *normalized*:
//! the divisor `d` must have its most-significant bit set, and `n_hi < d`
//! (otherwise the quotient overflows). Shift the numerator and divisor left
//! by `d.leading_zeros()` before calling if necessary.

use super::ct_types::{CTBool, Ct};
use super::multiplexers::{mux, mux32};

// ---------------------------------------------------------------------------
// u64 variant
// ---------------------------------------------------------------------------

/// Constant-time division: `(n_hi * 2^64 + n_lo) / d`.
///
/// Returns `(quotient, remainder)`.
///
/// # Preconditions
///
/// - `d` must be normalized: MSB of `d` must be set (`d >= 2^63`).
/// - `n_hi < d` — if `n_hi == d` the function substitutes `hi = 0` internally
///   (following the Constantine reference), and if `n_hi > d` the result is
///   undefined.
///
/// Maps from Constantine's `div2n1n` template in `ct_division.nim`.
#[inline]
#[must_use]
pub fn div2n1n(n_hi: Ct<u64>, n_lo: Ct<u64>, d: Ct<u64>) -> (Ct<u64>, Ct<u64>) {
    const BITS: u32 = u64::BITS; // 64

    let mut q = Ct::<u64>::new(0);

    // When n_hi == d, clamp hi to 0 to avoid quotient overflow.
    let hi_eq_d: CTBool<u64> = n_hi.ct_eq(d);
    let mut hi = mux(hi_eq_d, Ct::new(0), n_hi);
    let mut lo = n_lo;

    // Loop k from BITS-1 down to 1 (inclusive).
    for k in (1..BITS).rev() {
        let j = BITS - k;

        // w = (hi << j) | (lo >> k)
        let w = (hi << j) | (lo >> k);

        // ctl = (w >= d) | (hi >> k) != 0
        let w_ge_d: CTBool<u64> = !w.ct_lt(d);
        let hi_bit: CTBool<u64> = (hi >> k).is_non_zero();
        let ctl: CTBool<u64> = w_ge_d | hi_bit;

        // hi2 = (w - d) >> j
        let hi2 = (w - d) >> j;
        // lo2 = lo - (d << k)
        let lo2 = lo - (d << k);

        hi = mux(ctl, hi2, hi);
        lo = mux(ctl, lo2, lo);

        // q |= T(ctl) << k  —  ctl is 0 or 1
        q |= Ct::new(ctl.inner()) << k;
    }

    // Final step for k=0: check if remainder >= d (carry).
    let carry: CTBool<u64> = (!lo.ct_lt(d)) | hi.is_non_zero();
    q |= Ct::new(carry.inner());
    let r = mux(carry, lo - d, lo);

    (q, r)
}

// ---------------------------------------------------------------------------
// u32 variant
// ---------------------------------------------------------------------------

/// Constant-time division: `(n_hi * 2^32 + n_lo) / d`.
///
/// Returns `(quotient, remainder)`.
///
/// # Preconditions
///
/// - `d` must be normalized: MSB of `d` must be set (`d >= 2^31`).
/// - `n_hi < d` — if `n_hi == d` the function substitutes `hi = 0` internally
///   (following the Constantine reference), and if `n_hi > d` the result is
///   undefined.
///
/// Maps from Constantine's `div2n1n` template instantiated at `uint32`.
#[inline]
#[must_use]
pub fn div2n1n_u32(n_hi: Ct<u32>, n_lo: Ct<u32>, d: Ct<u32>) -> (Ct<u32>, Ct<u32>) {
    const BITS: u32 = u32::BITS; // 32

    let mut q = Ct::<u32>::new(0);

    let hi_eq_d: CTBool<u32> = n_hi.ct_eq(d);
    let mut hi = mux32(hi_eq_d, Ct::new(0), n_hi);
    let mut lo = n_lo;

    for k in (1..BITS).rev() {
        let j = BITS - k;

        let w = (hi << j) | (lo >> k);

        let w_ge_d: CTBool<u32> = !w.ct_lt(d);
        let hi_bit: CTBool<u32> = (hi >> k).is_non_zero();
        let ctl: CTBool<u32> = w_ge_d | hi_bit;

        let hi2 = (w - d) >> j;
        let lo2 = lo - (d << k);

        hi = mux32(ctl, hi2, hi);
        lo = mux32(ctl, lo2, lo);

        q |= Ct::new(ctl.inner()) << k;
    }

    let carry: CTBool<u32> = (!lo.ct_lt(d)) | hi.is_non_zero();
    q |= Ct::new(carry.inner());
    let r = mux32(carry, lo - d, lo);

    (q, r)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: normalize inputs and call div2n1n, then un-normalize remainder.
    fn div_u128_via_ct(num: u128, denom: u64) -> (u64, u64) {
        assert!(denom != 0, "divisor must be non-zero");
        let shift = denom.leading_zeros();
        let d_norm = denom << shift;
        // Shift 128-bit numerator left by `shift`.
        let num_shifted = num << u128::from(shift);
        // Intentional: take each 64-bit half of the shifted 128-bit value.
        // n_hi: upper half — shifting 128 right by 64 fits in u64.
        // n_lo: lower half — truncation is deliberate.
        let n_hi = (num_shifted >> 64) as u64;
        #[expect(clippy::cast_possible_truncation, reason = "intentional: lower 64 bits of double-width dividend")]
        let n_lo = num_shifted as u64;

        let (q, r_norm) = div2n1n(Ct::new(n_hi), Ct::new(n_lo), Ct::new(d_norm));
        // Un-normalize remainder.
        let r = r_norm.inner() >> shift;
        (q.inner(), r)
    }

    fn div_u64_via_ct(num: u64, denom: u32) -> (u32, u32) {
        assert!(denom != 0);
        let shift = denom.leading_zeros();
        let d_norm = denom << shift;
        let num_shifted = num << u64::from(shift);
        // n_hi: upper half — shifting 64 right by 32 fits in u32.
        // n_lo: lower half — truncation is deliberate.
        let n_hi = (num_shifted >> 32) as u32;
        #[expect(clippy::cast_possible_truncation, reason = "intentional: lower 32 bits of double-width dividend")]
        let n_lo = num_shifted as u32;

        let (q, r_norm) = div2n1n_u32(Ct::new(n_hi), Ct::new(n_lo), Ct::new(d_norm));
        let r = r_norm.inner() >> shift;
        (q.inner(), r)
    }

    // --- u64 basic ---

    #[test]
    fn div_100_by_7() {
        let (q, r) = div_u128_via_ct(100, 7);
        assert_eq!(q, 14, "quotient");
        assert_eq!(r, 2, "remainder");
    }

    #[test]
    fn div_zero_numerator() {
        let (q, r) = div_u128_via_ct(0, 7);
        assert_eq!(q, 0);
        assert_eq!(r, 0);
    }

    #[test]
    fn div_exact() {
        // 49 / 7 = 7 r 0
        let (q, r) = div_u128_via_ct(49, 7);
        assert_eq!(q, 7);
        assert_eq!(r, 0);
    }

    #[test]
    fn div_numerator_equals_denominator() {
        let (q, r) = div_u128_via_ct(13, 13);
        assert_eq!(q, 1);
        assert_eq!(r, 0);
    }

    #[test]
    fn div_numerator_less_than_denominator() {
        let (q, r) = div_u128_via_ct(5, 13);
        assert_eq!(q, 0);
        assert_eq!(r, 5);
    }

    #[test]
    fn div_large_cross_check() {
        // Cross-check against u128 native division for a range of values.
        let cases: &[(u128, u64)] = &[
            (1, 1),
            (u128::from(u64::MAX), 3),
            (u128::from(u64::MAX), u64::from(u32::MAX)),
            ((1u128 << 63) - 1, 0x8000_0000_0000_0001),
            (0x1234_5678_ABCD_EF01_u128, 0xFFFF_FFFF_FFFF_FFFF),
        ];
        for &(num, denom) in cases {
            // expected values fit in u64 because num < 2^64 in all cases above.
            #[expect(clippy::cast_possible_truncation, reason = "intentional: taking the low half of shifted double-width value")]
            let expected_q = (num / u128::from(denom)) as u64;
            #[expect(clippy::cast_possible_truncation, reason = "intentional: taking the low half of shifted double-width value")]
            let expected_r = (num % u128::from(denom)) as u64;
            let (q, r) = div_u128_via_ct(num, denom);
            assert_eq!(q, expected_q, "q mismatch for {num}/{denom}");
            assert_eq!(r, expected_r, "r mismatch for {num}/{denom}");
        }
    }

    #[test]
    fn div_normalized_directly() {
        // Call div2n1n directly with a pre-normalized divisor (MSB set).
        // Compute: (1 * 2^64 + 0) / 2^63 = 2 remainder 0, but n_hi must < d.
        // Use n_hi=0, n_lo=2^63, d=2^63+1 (normalized): result = 0 rem 2^63.
        let d = Ct::<u64>::new(0x8000_0000_0000_0001u64);
        let n_hi = Ct::<u64>::new(0);
        let n_lo = Ct::<u64>::new(0x8000_0000_0000_0000u64);
        let (q, r) = div2n1n(n_hi, n_lo, d);
        assert_eq!(q.inner(), 0);
        assert_eq!(r.inner(), 0x8000_0000_0000_0000u64);
    }

    #[test]
    fn div_n_hi_zero() {
        // n_hi = 0, so the double-width numerator is just n_lo.
        // Use already-normalized inputs: d has MSB set, n_hi < d.
        let d = Ct::<u64>::new(0xFFFF_FFFF_FFFF_FFFFu64);
        let n_lo = Ct::<u64>::new(0xFFFF_FFFF_FFFF_FFFEu64);
        let (q, r) = div2n1n(Ct::new(0), n_lo, d);
        assert_eq!(q.inner(), 0);
        assert_eq!(r.inner(), 0xFFFF_FFFF_FFFF_FFFEu64);
    }

    // --- u32 basic ---

    #[test]
    fn div32_100_by_7() {
        let (q, r) = div_u64_via_ct(100, 7);
        assert_eq!(q, 14);
        assert_eq!(r, 2);
    }

    #[test]
    fn div32_exact() {
        let (q, r) = div_u64_via_ct(49, 7);
        assert_eq!(q, 7);
        assert_eq!(r, 0);
    }

    #[test]
    fn div32_zero_numerator() {
        let (q, r) = div_u64_via_ct(0, 3);
        assert_eq!(q, 0);
        assert_eq!(r, 0);
    }

    #[test]
    fn div32_large_cross_check() {
        let cases: &[(u64, u32)] = &[
            (1, 1),
            (u64::from(u32::MAX), 3),
            (u64::from(u32::MAX), 0x8000_0001u32),
            (0x1234_5678u64, 0xFFFF_FFFFu32),
            ((1u64 << 31) - 1, 0x8000_0001u32),
        ];
        for &(num, denom) in cases {
            // expected values fit in u32 because num < 2^32 in all cases above.
            #[expect(clippy::cast_possible_truncation, reason = "intentional: taking the low half of shifted double-width value")]
            let expected_q = (num / u64::from(denom)) as u32;
            #[expect(clippy::cast_possible_truncation, reason = "intentional: taking the low half of shifted double-width value")]
            let expected_r = (num % u64::from(denom)) as u32;
            let (q, r) = div_u64_via_ct(num, denom);
            assert_eq!(q, expected_q, "q mismatch for {num}/{denom}");
            assert_eq!(r, expected_r, "r mismatch for {num}/{denom}");
        }
    }

    #[test]
    fn div32_normalized_directly() {
        // n_hi=0, n_lo < d, d has MSB set.
        let d = Ct::<u32>::new(0x8000_0001u32);
        let n_lo = Ct::<u32>::new(0x7FFF_FFFFu32);
        let (q, r) = div2n1n_u32(Ct::new(0), n_lo, d);
        assert_eq!(q.inner(), 0);
        assert_eq!(r.inner(), 0x7FFF_FFFFu32);
    }
}
