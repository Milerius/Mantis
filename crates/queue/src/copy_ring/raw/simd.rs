//! SIMD copy kernels and `DefaultCopyPolicy` for the copy-optimized ring.
//!
//! # Dispatch strategy
//!
//! `CopyDispatcher<T, N>` selects a kernel at compile time based on `N =
//! size_of::<T>()`:
//!
//! | `N` (bytes) | `x86_64` / `aarch64`      | other / Miri |
//! |-------------|---------------------------|--------------|
//! | ≤ 8         | `ptr::copy_nonoverlapping` | same         |
//! | 16          | 1 × 128-bit load/store    | scalar       |
//! | 32          | 2 × 128-bit               | scalar       |
//! | 48          | 3 × 128-bit               | scalar       |
//! | 64          | 4 × 128-bit               | scalar       |
//! | 17–31       | 2 loads + tail scalar     | scalar       |
//! | 33–47       | 3 loads + tail scalar     | scalar       |
//! | 49–63       | 4 loads + tail scalar     | scalar       |
//! | other       | `ptr::copy_nonoverlapping` | same         |
//!
//! Miri does not support SIMD intrinsics; the `#[cfg]` guards ensure it
//! always reaches the scalar fallback.

#![expect(unsafe_code, reason = "SIMD intrinsics require unsafe")]
// SIMD dispatch helpers (load128, store128, copy_N, copy_bucket) are called
// only through CopyDispatcher::copy which is invoked by SimdCopyPolicy and
// tests. The dead_code lint cannot trace through const-generic dispatch
// branches, so it incorrectly marks them as unused.
#![allow(dead_code)]

use core::marker::PhantomData;
use core::ptr;

use mantis_core::CopyPolicy;

// ── platform intrinsics ───────────────────────────────────────────────────────

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::{__m128i, _mm_loadu_si128, _mm_storeu_si128};

#[cfg(target_arch = "aarch64")]
use core::arch::aarch64::{uint8x16_t, vld1q_u8, vst1q_u8};

/// Load 16 bytes from an unaligned pointer.
///
/// # Safety
///
/// `src` must be valid for reads of 16 bytes.
#[cfg(target_arch = "x86_64")]
#[expect(
    clippy::inline_always,
    reason = "hot SIMD primitive — inlining is load-bearing"
)]
#[expect(
    clippy::cast_ptr_alignment,
    reason = "_mm_loadu_si128 performs an unaligned load — alignment is not required"
)]
#[inline(always)]
unsafe fn load128(src: *const u8) -> __m128i {
    // SAFETY: caller guarantees src is valid for 16-byte reads.
    unsafe { _mm_loadu_si128(src.cast::<__m128i>()) }
}

/// Store 16 bytes to an unaligned pointer.
///
/// # Safety
///
/// `dst` must be valid for writes of 16 bytes.
#[cfg(target_arch = "x86_64")]
#[expect(
    clippy::inline_always,
    reason = "hot SIMD primitive — inlining is load-bearing"
)]
#[expect(
    clippy::cast_ptr_alignment,
    reason = "_mm_storeu_si128 performs an unaligned store — alignment is not required"
)]
#[inline(always)]
unsafe fn store128(dst: *mut u8, v: __m128i) {
    // SAFETY: caller guarantees dst is valid for 16-byte writes.
    unsafe { _mm_storeu_si128(dst.cast::<__m128i>(), v) }
}

/// Load 16 bytes from an unaligned pointer.
///
/// # Safety
///
/// `src` must be valid for reads of 16 bytes.
#[cfg(target_arch = "aarch64")]
#[expect(
    clippy::inline_always,
    reason = "hot SIMD primitive — inlining is load-bearing"
)]
#[inline(always)]
unsafe fn load128(src: *const u8) -> uint8x16_t {
    // SAFETY: caller guarantees src is valid for 16-byte reads.
    unsafe { vld1q_u8(src) }
}

/// Store 16 bytes to an unaligned pointer.
///
/// # Safety
///
/// `dst` must be valid for writes of 16 bytes.
#[cfg(target_arch = "aarch64")]
#[expect(
    clippy::inline_always,
    reason = "hot SIMD primitive — inlining is load-bearing"
)]
#[inline(always)]
unsafe fn store128(dst: *mut u8, v: uint8x16_t) {
    // SAFETY: caller guarantees dst is valid for 16-byte writes.
    unsafe { vst1q_u8(dst, v) }
}

// ── exact-size kernels ────────────────────────────────────────────────────────

/// Generate an exact-size SIMD copy kernel for `$chunks` × 16 bytes.
///
/// The generated function is only compiled on `x86_64` and `aarch64`.
macro_rules! define_copy_exact {
    ($name:ident, $chunks:expr) => {
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        #[inline(always)]
        unsafe fn $name(dst: *mut u8, src: *const u8) {
            let mut i = 0usize;
            while i < $chunks {
                // SAFETY: caller guarantees src and dst are valid for
                // $chunks * 16 bytes.  Each iteration accesses a
                // non-overlapping 16-byte window.
                let v = unsafe { load128(src.add(i * 16)) };
                // SAFETY: same as above.
                unsafe { store128(dst.add(i * 16), v) };
                i += 1;
            }
        }
    };
}

define_copy_exact!(copy_16, 1); // 16 bytes
define_copy_exact!(copy_32, 2); // 32 bytes
define_copy_exact!(copy_48, 3); // 48 bytes
define_copy_exact!(copy_64, 4); // 64 bytes

// ── bucket fallback ───────────────────────────────────────────────────────────

/// Copy `N` bytes by copying a `BASE`-byte SIMD prefix then a scalar tail.
///
/// Used for sizes that don't land on a 16-byte boundary.
///
/// # Safety
///
/// `src` and `dst` must both be valid for `N` bytes. `BASE < N`.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[expect(
    clippy::inline_always,
    reason = "hot SIMD kernel — inlining eliminates call overhead"
)]
#[inline(always)]
unsafe fn copy_bucket<const BASE: usize, const N: usize>(dst: *mut u8, src: *const u8) {
    // SAFETY: caller guarantees src/dst valid for N >= BASE+1 bytes.
    unsafe {
        match BASE {
            16 => copy_16(dst, src),
            32 => copy_32(dst, src),
            48 => copy_48(dst, src),
            _ => {}
        }
        // SAFETY: N > BASE; remaining tail bytes are within the valid range.
        ptr::copy_nonoverlapping(src.add(BASE), dst.add(BASE), N - BASE);
    }
}

// ── CopyDispatcher ────────────────────────────────────────────────────────────

/// Compile-time dispatcher that selects the fastest copy kernel for `T`.
///
/// `N` must equal `core::mem::size_of::<T>()`. It is a separate const
/// parameter because `size_of` in a const-generic position requires
/// `#![feature(generic_const_exprs)]` on nightly; callers supply it
/// explicitly so this struct compiles on stable too.
pub(crate) struct CopyDispatcher<T, const N: usize>(PhantomData<T>);

impl<T: Copy, const N: usize> CopyDispatcher<T, N> {
    /// Copy one value of type `T` from `src` to `dst` using the fastest
    /// available kernel for `N = size_of::<T>()`.
    ///
    /// # Safety
    ///
    /// - `src` must be valid, aligned, and initialized for reads of `T`.
    /// - `dst` must be valid and aligned for writes of `T`.
    /// - `src` and `dst` must not overlap.
    #[expect(
        clippy::inline_always,
        reason = "dispatch function must inline so the optimizer can constant-fold N"
    )]
    #[inline(always)]
    pub(crate) unsafe fn copy(dst: *mut T, src: *const T) {
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        {
            if N <= 8 {
                // SAFETY: T: Copy; N <= 8 means a single ptr::copy is safe.
                unsafe { ptr::copy_nonoverlapping(src, dst, 1) };
                return;
            }
            if N == 16 {
                // SAFETY: N == 16; src/dst valid for 16 bytes per contract.
                unsafe { copy_16(dst.cast::<u8>(), src.cast::<u8>()) };
                return;
            }
            if N == 32 {
                // SAFETY: N == 32; src/dst valid for 32 bytes per contract.
                unsafe { copy_32(dst.cast::<u8>(), src.cast::<u8>()) };
                return;
            }
            if N == 48 {
                // SAFETY: N == 48; src/dst valid for 48 bytes per contract.
                unsafe { copy_48(dst.cast::<u8>(), src.cast::<u8>()) };
                return;
            }
            if N == 64 {
                // SAFETY: N == 64; src/dst valid for 64 bytes per contract.
                unsafe { copy_64(dst.cast::<u8>(), src.cast::<u8>()) };
                return;
            }
            if N < 32 {
                // SAFETY: 8 < N < 32; BASE=16 < N; src/dst valid for N bytes.
                unsafe { copy_bucket::<16, N>(dst.cast::<u8>(), src.cast::<u8>()) };
                return;
            }
            if N < 48 {
                // SAFETY: 32 <= N < 48; BASE=32 < N; src/dst valid for N bytes.
                unsafe { copy_bucket::<32, N>(dst.cast::<u8>(), src.cast::<u8>()) };
                return;
            }
            if N < 64 {
                // SAFETY: 48 <= N < 64; BASE=48 < N; src/dst valid for N bytes.
                unsafe { copy_bucket::<48, N>(dst.cast::<u8>(), src.cast::<u8>()) };
                return;
            }
        }
        // Scalar fallback: other architectures, Miri, and sizes >= 64 that
        // didn't match an exact kernel above.
        // SAFETY: caller invariants ensure src/dst valid, aligned, non-overlapping.
        unsafe { ptr::copy_nonoverlapping(src, dst, 1) };
    }
}

// ── DefaultCopyPolicy ─────────────────────────────────────────────────────────

/// Default copy policy: uses `ptr::copy_nonoverlapping` for portability.
///
/// On `x86_64` and `aarch64`, the optimizer will lower fixed-size copies to
/// SIMD moves at `-O2` and above. For guaranteed SIMD at all optimization
/// levels, use `SimdCopyPolicy` (nightly only).
///
/// This is a zero-sized type used for static dispatch only.
pub struct DefaultCopyPolicy;

impl<T: Copy> CopyPolicy<T> for DefaultCopyPolicy {
    #[inline]
    fn copy_in(dst: *mut T, src: *const T) {
        // SAFETY: Caller guarantees dst/src are valid, aligned, and the slot
        // is logically unoccupied (SPSC producer exclusivity). src and dst
        // do not overlap (ring slot vs caller-owned memory).
        unsafe { ptr::copy_nonoverlapping(src, dst, 1) };
    }

    #[inline]
    fn copy_out(dst: *mut T, src: *const T) {
        // SAFETY: Caller guarantees src is valid, aligned, and points to an
        // occupied initialized slot (SPSC consumer exclusivity). dst and src
        // do not overlap.
        unsafe { ptr::copy_nonoverlapping(src, dst, 1) };
    }
}

// ── SimdCopyPolicy (nightly only) ─────────────────────────────────────────────

/// SIMD-accelerated copy policy using explicit 128-bit load/store kernels.
///
/// Requires nightly (`generic_const_exprs` feature). Enables SIMD at all
/// optimization levels, including debug builds.
///
/// `SimdCopyPolicy` implements `CopyPolicy<T>` only for types where
/// `[(); size_of::<T>()]` is constructible — i.e., all `Copy` types.
///
/// This is a zero-sized type used for static dispatch only.
#[cfg(feature = "nightly")]
pub struct SimdCopyPolicy;

#[cfg(feature = "nightly")]
impl<T: Copy> CopyPolicy<T> for SimdCopyPolicy
where
    [(); core::mem::size_of::<T>()]:,
{
    #[inline]
    fn copy_in(dst: *mut T, src: *const T) {
        // SAFETY: Caller guarantees dst/src are valid, aligned, non-overlapping,
        // and the slot is logically unoccupied (SPSC producer exclusivity).
        // CopyDispatcher reads exactly size_of::<T>() bytes from src and
        // writes to dst.
        unsafe { CopyDispatcher::<T, { core::mem::size_of::<T>() }>::copy(dst, src) };
    }

    #[inline]
    fn copy_out(dst: *mut T, src: *const T) {
        // SAFETY: Caller guarantees src is valid, aligned, initialized, and
        // points to an occupied slot (SPSC consumer exclusivity).
        unsafe { CopyDispatcher::<T, { core::mem::size_of::<T>() }>::copy(dst, src) };
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use core::ptr;

    use super::*;

    /// Verify that `DefaultCopyPolicy` round-trips a value correctly.
    macro_rules! test_copy_roundtrip {
        ($test_name:ident, $ty:ty, $value:expr) => {
            #[test]
            fn $test_name() {
                let src: $ty = $value;
                let mut dst: core::mem::MaybeUninit<$ty> = core::mem::MaybeUninit::uninit();
                // SAFETY: src is a valid initialized local; dst is a valid
                // MaybeUninit allocation. addr_of! avoids creating a reference.
                DefaultCopyPolicy::copy_in(dst.as_mut_ptr(), ptr::addr_of!(src));
                // SAFETY: copy_in has initialized dst.
                let result = unsafe { dst.assume_init() };
                assert_eq!(result, src);
            }
        };
    }

    // 8-byte scalar path
    test_copy_roundtrip!(roundtrip_u64, u64, 0xDEAD_BEEF_CAFE_1234_u64);

    // 16-byte exact kernel
    test_copy_roundtrip!(roundtrip_16, [u8; 16], [0xAB_u8; 16]);

    // 24-byte bucket path (BASE=16, tail=8)
    test_copy_roundtrip!(roundtrip_24, [u8; 24], [0xCD_u8; 24]);

    // 32-byte exact kernel
    test_copy_roundtrip!(roundtrip_32, [u8; 32], [0x12_u8; 32]);

    // 48-byte exact kernel
    test_copy_roundtrip!(roundtrip_48, [u8; 48], [0x34_u8; 48]);

    // 56-byte bucket path (BASE=48, tail=8)
    test_copy_roundtrip!(roundtrip_56, [u8; 56], [0x56_u8; 56]);

    // 64-byte exact kernel
    test_copy_roundtrip!(roundtrip_64, [u8; 64], [0x78_u8; 64]);

    // 128-byte scalar fallback
    test_copy_roundtrip!(roundtrip_128, [u8; 128], [0x9A_u8; 128]);

    // copy_out symmetry — verify the out direction also round-trips
    #[test]
    fn copy_out_roundtrip_u64() {
        let src: u64 = 0x1111_2222_3333_4444;
        let mut dst = core::mem::MaybeUninit::<u64>::uninit();
        // SAFETY: src is a valid initialized local; dst is a valid MaybeUninit.
        DefaultCopyPolicy::copy_out(dst.as_mut_ptr(), ptr::addr_of!(src));
        // SAFETY: copy_out has initialized dst.
        let result = unsafe { dst.assume_init() };
        assert_eq!(result, src);
    }

    // CopyDispatcher directly — spot-check 32-byte path
    #[test]
    fn dispatcher_32_bytes() {
        let src = [0xFFu8; 32];
        let mut dst = [0u8; 32];
        // SAFETY: src and dst are valid, aligned, non-overlapping 32-byte arrays.
        unsafe {
            CopyDispatcher::<[u8; 32], 32>::copy(dst.as_mut_ptr().cast(), src.as_ptr().cast());
        }
        assert_eq!(dst, src);
    }

    // SimdCopyPolicy tests (nightly only) ─────────────────────────────────────

    /// Verify that `SimdCopyPolicy` round-trips a value correctly.
    #[cfg(feature = "nightly")]
    macro_rules! test_simd_roundtrip {
        ($test_name:ident, $ty:ty, $value:expr) => {
            #[test]
            fn $test_name() {
                let src: $ty = $value;
                let mut dst: core::mem::MaybeUninit<$ty> = core::mem::MaybeUninit::uninit();
                SimdCopyPolicy::copy_in(dst.as_mut_ptr(), ptr::addr_of!(src));
                // SAFETY: copy_in has initialized dst.
                let result = unsafe { dst.assume_init() };
                assert_eq!(result, src);
            }
        };
    }

    #[cfg(feature = "nightly")]
    test_simd_roundtrip!(simd_roundtrip_u64, u64, 0xDEAD_BEEF_CAFE_1234_u64);
    #[cfg(feature = "nightly")]
    test_simd_roundtrip!(simd_roundtrip_16, [u8; 16], [0xAB_u8; 16]);
    #[cfg(feature = "nightly")]
    test_simd_roundtrip!(simd_roundtrip_32, [u8; 32], [0x12_u8; 32]);
    #[cfg(feature = "nightly")]
    test_simd_roundtrip!(simd_roundtrip_48, [u8; 48], [0x34_u8; 48]);
    #[cfg(feature = "nightly")]
    test_simd_roundtrip!(simd_roundtrip_64, [u8; 64], [0x78_u8; 64]);
    #[cfg(feature = "nightly")]
    test_simd_roundtrip!(simd_roundtrip_24, [u8; 24], [0xCD_u8; 24]);
    #[cfg(feature = "nightly")]
    test_simd_roundtrip!(simd_roundtrip_56, [u8; 56], [0x56_u8; 56]);
    #[cfg(feature = "nightly")]
    test_simd_roundtrip!(simd_roundtrip_128, [u8; 128], [0x9A_u8; 128]);

    #[cfg(feature = "nightly")]
    #[test]
    fn simd_copy_out_roundtrip_u64() {
        let src: u64 = 0x1111_2222_3333_4444;
        let mut dst = core::mem::MaybeUninit::<u64>::uninit();
        SimdCopyPolicy::copy_out(dst.as_mut_ptr(), ptr::addr_of!(src));
        // SAFETY: copy_out has initialized dst.
        let result = unsafe { dst.assume_init() };
        assert_eq!(result, src);
    }
}
