//! Constant-time multiplexers: select, conditional copy, and secret lookup.
//!
//! Maps from Constantine's `multiplexers.nim` and `ct_routines.nim`.
//!
//! All operations are branch-free on secret data. The `x86_64` asm paths use
//! `cmovz`/`cmovnz` which are data-oblivious in all known microarchitectures.

// The crate root has `#![deny(unsafe_code)]`; the asm implementations below
// require unsafe blocks. We allow unsafe only in this file.
#![allow(unsafe_code)]

use super::ct_types::{CTBool, Ct};

// ---------------------------------------------------------------------------
// Portable formula helpers (inlined, not exported)
// ---------------------------------------------------------------------------

/// Portable mux formula: `y ^ (-(T(ctl)) & (x ^ y))`.
///
/// When ctl==1 (true): mask = all-ones  → result = y ^ (x ^ y) = x.
/// When ctl==0 (false): mask = 0        → result = y ^ 0        = y.
macro_rules! mux_formula {
    ($ctl:expr, $x:expr, $y:expr, $t:ty) => {{
        let mask: Ct<$t> = -$ctl.0;
        $y ^ (mask & ($x ^ $y))
    }};
}

// ---------------------------------------------------------------------------
// Public API — u64
// ---------------------------------------------------------------------------

/// Constant-time select for `u64`.
///
/// Returns `x` when `ctl` is true, `y` when `ctl` is false, without
/// branching on `ctl`.
#[must_use]
#[inline]
pub fn mux(ctl: CTBool<u64>, x: Ct<u64>, y: Ct<u64>) -> Ct<u64> {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: `ctl.inner()` is 0 or 1 (CTBool invariant). `test` sets ZF
        // when ctl==0 (false). `cmovz` moves `y` into `muxed` only when
        // ZF is set, i.e. when ctl==0. So result is x when ctl==1, y when
        // ctl==0. No memory accessed; all operands are registers. No UB.
        let mut muxed = x.0;
        unsafe {
            core::arch::asm!(
                "test {ctl}, {ctl}",
                "cmovz {muxed}, {y}",
                ctl  = in(reg) ctl.inner(),
                muxed = inlateout(reg) muxed,
                y    = in(reg) y.0,
                options(pure, nomem, nostack),
            );
        }
        Ct(muxed)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        mux_formula!(ctl, x, y, u64)
    }
}

/// Constant-time select for `u32`.
#[must_use]
#[inline]
pub fn mux32(ctl: CTBool<u32>, x: Ct<u32>, y: Ct<u32>) -> Ct<u32> {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: same invariant as `mux` but for 32-bit registers.
        // The `:e` modifier selects the 32-bit sub-register (eax, etc.).
        let mut muxed = x.0;
        unsafe {
            core::arch::asm!(
                "test {ctl:e}, {ctl:e}",
                "cmovz {muxed:e}, {y:e}",
                ctl   = in(reg) ctl.inner(),
                muxed = inlateout(reg) muxed,
                y     = in(reg) y.0,
                options(pure, nomem, nostack),
            );
        }
        Ct(muxed)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        mux_formula!(ctl, x, y, u32)
    }
}

/// Constant-time select for `usize`.
#[must_use]
#[inline]
pub fn mux_usize(ctl: CTBool<usize>, x: Ct<usize>, y: Ct<usize>) -> Ct<usize> {
    mux_formula!(ctl, x, y, usize)
}

/// Constant-time select on `CTBool<u64>` values.
///
/// Returns `x` when `ctl` is true, `y` when `ctl` is false.
#[must_use]
#[inline]
pub fn mux_bool(ctl: CTBool<u64>, x: CTBool<u64>, y: CTBool<u64>) -> CTBool<u64> {
    CTBool(mux(ctl, x.0, y.0))
}

/// Constant-time select on `CTBool<u32>` values.
#[must_use]
#[inline]
pub fn mux_bool32(ctl: CTBool<u32>, x: CTBool<u32>, y: CTBool<u32>) -> CTBool<u32> {
    CTBool(mux32(ctl, x.0, y.0))
}

/// Constant-time select on `CTBool<usize>` values.
#[must_use]
#[inline]
pub fn mux_bool_usize(ctl: CTBool<usize>, x: CTBool<usize>, y: CTBool<usize>) -> CTBool<usize> {
    CTBool(mux_usize(ctl, x.0, y.0))
}

// ---------------------------------------------------------------------------
// Public API — ccopy
// ---------------------------------------------------------------------------

/// Constant-time conditional copy for `u64`.
///
/// Sets `*x = y` when `ctl` is true; leaves `*x` unchanged otherwise.
#[inline]
pub fn ccopy(ctl: CTBool<u64>, x: &mut Ct<u64>, y: Ct<u64>) {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: `ctl.inner()` is 0 or 1. `test` sets ZF when ctl==0;
        // `cmovnz` moves `y` into the register holding `x.0` only when
        // ZF is clear (ctl==1, true). The reference `x` is valid and
        // aligned. No UB.
        unsafe {
            core::arch::asm!(
                "test {ctl}, {ctl}",
                "cmovnz {cur}, {y}",
                ctl = in(reg) ctl.inner(),
                cur = inlateout(reg) x.0,
                y   = in(reg) y.0,
                options(nomem, nostack),
            );
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        *x = mux(ctl, y, *x);
    }
}

/// Constant-time conditional copy for `u32`.
#[inline]
pub fn ccopy32(ctl: CTBool<u32>, x: &mut Ct<u32>, y: Ct<u32>) {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: same as `ccopy` for 32-bit operands.
        unsafe {
            core::arch::asm!(
                "test {ctl:e}, {ctl:e}",
                "cmovnz {cur:e}, {y:e}",
                ctl = in(reg) ctl.inner(),
                cur = inlateout(reg) x.0,
                y   = in(reg) y.0,
                options(nomem, nostack),
            );
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        *x = mux32(ctl, y, *x);
    }
}

/// Constant-time conditional copy for `usize`.
// No x86_64 asm path for usize — pointer width varies by platform.
#[inline]
pub fn ccopy_usize(ctl: CTBool<usize>, x: &mut Ct<usize>, y: Ct<usize>) {
    *x = mux_usize(ctl, y, *x);
}

// ---------------------------------------------------------------------------
// secret_lookup
// ---------------------------------------------------------------------------

/// Constant-time table lookup.
///
/// Scans the entire `table` and returns the element at `index` without
/// revealing `index` through timing. The scan is O(n) — no early exit.
#[must_use]
#[inline]
pub fn secret_lookup(table: &[u64], index: Ct<usize>) -> Ct<u64> {
    let mut val = Ct::<u64>::new(0);
    for (i, &entry) in table.iter().enumerate() {
        let selector = Ct::<usize>::new(i).ct_eq(index);
        // CTBool values are always 0 or 1, so the as-cast from usize to
        // u64 preserves the invariant and is safe for cross-domain use.
        let sel64 = CTBool::<u64>(Ct(selector.inner() as u64));
        ccopy(sel64, &mut val, Ct::new(entry));
    }
    val
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- mux u64 ---

    #[test]
    fn mux_true_returns_x() {
        assert_eq!(
            mux(CTBool::<u64>::ctrue(), Ct::new(42u64), Ct::new(99u64)).inner(),
            42u64
        );
    }

    #[test]
    fn mux_false_returns_y() {
        assert_eq!(
            mux(CTBool::<u64>::cfalse(), Ct::new(42u64), Ct::new(99u64)).inner(),
            99u64
        );
    }

    #[test]
    fn mux_boundary_values() {
        assert_eq!(
            mux(CTBool::<u64>::ctrue(), Ct::new(0u64), Ct::new(u64::MAX)).inner(),
            0u64
        );
        assert_eq!(
            mux(CTBool::<u64>::cfalse(), Ct::new(0u64), Ct::new(u64::MAX)).inner(),
            u64::MAX
        );
        assert_eq!(
            mux(CTBool::<u64>::ctrue(), Ct::new(u64::MAX), Ct::new(0u64)).inner(),
            u64::MAX
        );
    }

    #[test]
    fn mux_same_x_y() {
        assert_eq!(
            mux(CTBool::<u64>::ctrue(), Ct::new(7u64), Ct::new(7u64)).inner(),
            7u64
        );
        assert_eq!(
            mux(CTBool::<u64>::cfalse(), Ct::new(7u64), Ct::new(7u64)).inner(),
            7u64
        );
    }

    // --- mux u32 ---

    #[test]
    fn mux32_true_returns_x() {
        assert_eq!(
            mux32(CTBool::<u32>::ctrue(), Ct::new(1u32), Ct::new(2u32)).inner(),
            1u32
        );
    }

    #[test]
    fn mux32_false_returns_y() {
        assert_eq!(
            mux32(CTBool::<u32>::cfalse(), Ct::new(1u32), Ct::new(2u32)).inner(),
            2u32
        );
    }

    #[test]
    fn mux32_boundary_values() {
        assert_eq!(
            mux32(CTBool::<u32>::ctrue(), Ct::new(u32::MAX), Ct::new(0u32)).inner(),
            u32::MAX
        );
        assert_eq!(
            mux32(CTBool::<u32>::cfalse(), Ct::new(u32::MAX), Ct::new(0u32)).inner(),
            0u32
        );
    }

    // --- mux usize ---

    #[test]
    fn mux_usize_selects_correctly() {
        assert_eq!(
            mux_usize(
                CTBool::<usize>::ctrue(),
                Ct::new(100usize),
                Ct::new(200usize)
            )
            .inner(),
            100usize
        );
        assert_eq!(
            mux_usize(
                CTBool::<usize>::cfalse(),
                Ct::new(100usize),
                Ct::new(200usize)
            )
            .inner(),
            200usize
        );
    }

    // --- mux_bool ---

    #[test]
    fn mux_bool_true_selects_x() {
        let result = mux_bool(
            CTBool::<u64>::ctrue(),
            CTBool::<u64>::ctrue(),
            CTBool::<u64>::cfalse(),
        );
        assert_eq!(result.inner(), 1u64);
    }

    #[test]
    fn mux_bool_false_selects_y() {
        let result = mux_bool(
            CTBool::<u64>::cfalse(),
            CTBool::<u64>::ctrue(),
            CTBool::<u64>::cfalse(),
        );
        assert_eq!(result.inner(), 0u64);
    }

    // --- mux_bool32 ---

    #[test]
    fn mux_bool32_selects_correctly() {
        let t = CTBool::<u32>::ctrue();
        let f = CTBool::<u32>::cfalse();
        assert_eq!(mux_bool32(t, t, f).inner(), 1u32);
        assert_eq!(mux_bool32(f, t, f).inner(), 0u32);
    }

    // --- mux_bool_usize ---

    #[test]
    fn mux_bool_usize_selects_correctly() {
        let t = CTBool::<usize>::ctrue();
        let f = CTBool::<usize>::cfalse();
        assert_eq!(mux_bool_usize(t, t, f).inner(), 1usize);
        assert_eq!(mux_bool_usize(f, t, f).inner(), 0usize);
    }

    // --- x86_64: portable formula agrees with asm ---

    #[test]
    #[cfg(target_arch = "x86_64")]
    fn mux_asm_matches_portable_u64() {
        let cases: &[(u64, u64)] = &[
            (0, 0),
            (0, u64::MAX),
            (u64::MAX, 0),
            (u64::MAX, u64::MAX),
            (0xDEAD_BEEF_CAFE_0000, 0x1234_5678_9ABC_DEF0),
        ];
        for &(xv, yv) in cases {
            let x = Ct::new(xv);
            let y = Ct::new(yv);
            let portable_t = mux_formula!(CTBool::<u64>::ctrue(), x, y, u64).inner();
            let portable_f = mux_formula!(CTBool::<u64>::cfalse(), x, y, u64).inner();
            assert_eq!(
                mux(CTBool::<u64>::ctrue(), x, y).inner(),
                portable_t,
                "asm/portable mismatch: true x={xv} y={yv}"
            );
            assert_eq!(
                mux(CTBool::<u64>::cfalse(), x, y).inner(),
                portable_f,
                "asm/portable mismatch: false x={xv} y={yv}"
            );
        }
    }

    #[test]
    #[cfg(target_arch = "x86_64")]
    fn mux32_asm_matches_portable() {
        let cases: &[(u32, u32)] = &[
            (0, 0),
            (0, u32::MAX),
            (u32::MAX, 0),
            (u32::MAX, u32::MAX),
            (0xDEAD_BEEF, 0x1234_5678),
        ];
        for &(xv, yv) in cases {
            let x = Ct::new(xv);
            let y = Ct::new(yv);
            let portable_t = mux_formula!(CTBool::<u32>::ctrue(), x, y, u32).inner();
            let portable_f = mux_formula!(CTBool::<u32>::cfalse(), x, y, u32).inner();
            assert_eq!(
                mux32(CTBool::<u32>::ctrue(), x, y).inner(),
                portable_t,
                "asm/portable mismatch: true x={xv} y={yv}"
            );
            assert_eq!(
                mux32(CTBool::<u32>::cfalse(), x, y).inner(),
                portable_f,
                "asm/portable mismatch: false x={xv} y={yv}"
            );
        }
    }

    // --- ccopy u64 ---

    #[test]
    fn ccopy_true_overwrites() {
        let mut x = Ct::new(10u64);
        ccopy(CTBool::<u64>::ctrue(), &mut x, Ct::new(99u64));
        assert_eq!(x.inner(), 99u64);
    }

    #[test]
    fn ccopy_false_preserves() {
        let mut x = Ct::new(10u64);
        ccopy(CTBool::<u64>::cfalse(), &mut x, Ct::new(99u64));
        assert_eq!(x.inner(), 10u64);
    }

    #[test]
    fn ccopy_same_value_idempotent() {
        let mut x = Ct::new(42u64);
        ccopy(CTBool::<u64>::ctrue(), &mut x, Ct::new(42u64));
        assert_eq!(x.inner(), 42u64);
    }

    // --- ccopy u32 ---

    #[test]
    fn ccopy32_true_overwrites() {
        let mut x = Ct::new(1u32);
        ccopy32(CTBool::<u32>::ctrue(), &mut x, Ct::new(99u32));
        assert_eq!(x.inner(), 99u32);
    }

    #[test]
    fn ccopy32_false_preserves() {
        let mut x = Ct::new(1u32);
        ccopy32(CTBool::<u32>::cfalse(), &mut x, Ct::new(99u32));
        assert_eq!(x.inner(), 1u32);
    }

    #[test]
    fn ccopy32_boundary_max() {
        let mut x = Ct::new(0u32);
        ccopy32(CTBool::<u32>::ctrue(), &mut x, Ct::new(u32::MAX));
        assert_eq!(x.inner(), u32::MAX);
        ccopy32(CTBool::<u32>::cfalse(), &mut x, Ct::new(0u32));
        assert_eq!(x.inner(), u32::MAX);
    }

    // --- ccopy usize ---

    #[test]
    fn ccopy_usize_true_overwrites() {
        let mut x = Ct::new(1usize);
        ccopy_usize(CTBool::<usize>::ctrue(), &mut x, Ct::new(99usize));
        assert_eq!(x.inner(), 99usize);
    }

    #[test]
    fn ccopy_usize_false_preserves() {
        let mut x = Ct::new(1usize);
        ccopy_usize(CTBool::<usize>::cfalse(), &mut x, Ct::new(99usize));
        assert_eq!(x.inner(), 1usize);
    }

    // --- secret_lookup ---

    #[test]
    fn secret_lookup_all_indices() {
        let table: &[u64] = &[10, 20, 30, 40, 50];
        for (i, &expected) in table.iter().enumerate() {
            assert_eq!(
                secret_lookup(table, Ct::new(i)).inner(),
                expected,
                "index {i}"
            );
        }
    }

    #[test]
    fn secret_lookup_single_element() {
        let table: &[u64] = &[0xDEAD_BEEF_CAFE_1234];
        assert_eq!(
            secret_lookup(table, Ct::new(0usize)).inner(),
            0xDEAD_BEEF_CAFE_1234u64
        );
    }

    #[test]
    fn secret_lookup_empty_returns_zero() {
        // No iterations — val stays at the default 0.
        let result = secret_lookup(&[], Ct::new(0usize));
        assert_eq!(result.inner(), 0u64);
    }

    #[test]
    fn secret_lookup_boundary_values() {
        let table: &[u64] = &[0, u64::MAX, u64::MAX / 2, 1];
        assert_eq!(secret_lookup(table, Ct::new(0usize)).inner(), 0u64);
        assert_eq!(secret_lookup(table, Ct::new(1usize)).inner(), u64::MAX);
        assert_eq!(secret_lookup(table, Ct::new(2usize)).inner(), u64::MAX / 2);
        assert_eq!(secret_lookup(table, Ct::new(3usize)).inner(), 1u64);
    }
}
