//! Compiler optimization hints: prefetch for cache management.
//!
//! Maps from Constantine's `compiler_optim_hints.nim`.

#![expect(unsafe_code, reason = "compiler intrinsics require unsafe")]

/// Prefetch direction.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefetchRW {
    /// Prefetch for a read operation.
    Read = 0,
    /// Prefetch for a write operation.
    Write = 1,
}

/// Prefetch temporal locality hint.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefetchLocality {
    /// Data can be discarded from CPU cache after access.
    NoTemporal = 0,
    /// L1 cache eviction level.
    Low = 1,
    /// L2 cache eviction level.
    Moderate = 2,
    /// Data should be left in all levels of cache.
    High = 3,
}

/// Prefetch a cache line containing `ptr`.
///
/// This is a hint — the CPU may ignore it. On platforms without
/// prefetch support, this is a no-op.
#[inline]
pub fn prefetch<T>(ptr: *const T, rw: PrefetchRW, locality: PrefetchLocality) {
    #[cfg(target_arch = "x86_64")]
    {
        use core::arch::x86_64::{
            _MM_HINT_ET0, _MM_HINT_NTA, _MM_HINT_T0, _MM_HINT_T1, _MM_HINT_T2, _mm_prefetch,
        };
        let hint = ptr.cast::<i8>();
        // SAFETY: prefetch is a hint and never faults, even on invalid addresses.
        // The locality must be a compile-time constant for _mm_prefetch.
        unsafe {
            match (rw, locality) {
                // Write prefetch: use ET0 (exclusive) to bring line in Modified
                // state, avoiding the subsequent RFO on the actual store.
                (PrefetchRW::Write, PrefetchLocality::High) => {
                    _mm_prefetch(hint, _MM_HINT_ET0);
                }
                // Non-High write localities fall through to read hints — ET0
                // only exists as a single locality level on x86; for other
                // localities we use the read hint as a reasonable fallback.
                (_, PrefetchLocality::NoTemporal) => _mm_prefetch(hint, _MM_HINT_NTA),
                (_, PrefetchLocality::Low) => _mm_prefetch(hint, _MM_HINT_T2),
                (_, PrefetchLocality::Moderate) => _mm_prefetch(hint, _MM_HINT_T1),
                (_, PrefetchLocality::High) => _mm_prefetch(hint, _MM_HINT_T0),
            }
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        let addr = ptr.cast::<u8>();
        // SAFETY: PRFM is a hint instruction — it never faults and has no
        // side effects beyond cache management. options(nostack, preserves_flags)
        // tells LLVM it doesn't touch the stack or condition flags.
        unsafe {
            match rw {
                PrefetchRW::Read => {
                    core::arch::asm!(
                        "prfm pldl1keep, [{ptr}]",
                        ptr = in(reg) addr,
                        options(nostack, preserves_flags),
                    );
                }
                PrefetchRW::Write => {
                    core::arch::asm!(
                        "prfm pstl1keep, [{ptr}]",
                        ptr = in(reg) addr,
                        options(nostack, preserves_flags),
                    );
                }
            }
        }
        let _ = locality; // Locality encoded in instruction mnemonic (L1KEEP)
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let _ = (ptr, rw, locality);
    }
}

/// Prefetch a large value spanning multiple cache lines.
///
/// Prefetches up to `max_lines` cache lines (0 = all lines covering T).
#[inline]
pub fn prefetch_large<T>(
    ptr: *const T,
    rw: PrefetchRW,
    locality: PrefetchLocality,
    max_lines: usize,
) {
    let span = core::mem::size_of::<T>() / 64; // 64-byte cache lines
    let n = if max_lines == 0 {
        span
    } else {
        span.min(max_lines)
    };
    for i in 0..n {
        // SAFETY: pointer arithmetic for prefetch hint; the resulting pointer
        // is never dereferenced, only passed to prefetch which is a no-op hint.
        let line_ptr = unsafe { ptr.cast::<u8>().add(i * 64) };
        prefetch(line_ptr, rw, locality);
    }
}

#[cfg(test)]
mod tests {
    use super::{PrefetchLocality, PrefetchRW, prefetch, prefetch_large};

    #[test]
    fn prefetch_does_not_crash() {
        let value: u64 = 42;
        prefetch(&raw const value, PrefetchRW::Read, PrefetchLocality::High);
        prefetch(
            &raw const value,
            PrefetchRW::Write,
            PrefetchLocality::NoTemporal,
        );
    }

    #[test]
    fn prefetch_large_handles_big_type() {
        let big: [u8; 512] = [0u8; 512];
        prefetch_large(
            big.as_ptr(),
            PrefetchRW::Read,
            PrefetchLocality::Moderate,
            0,
        );
        prefetch_large(big.as_ptr(), PrefetchRW::Read, PrefetchLocality::Low, 2);
    }

    #[test]
    fn prefetch_write_does_not_crash() {
        let mut value: u64 = 42;
        prefetch(&raw const value, PrefetchRW::Write, PrefetchLocality::High);
        // Verify write after prefetch still works
        value = 99;
        assert_eq!(value, 99);
    }

    #[test]
    fn prefetch_read_does_not_crash_stack_array() {
        let arr = [0u8; 128];
        prefetch(arr.as_ptr(), PrefetchRW::Read, PrefetchLocality::High);
    }

    #[test]
    fn enum_values() {
        assert_eq!(PrefetchRW::Read as i32, 0);
        assert_eq!(PrefetchRW::Write as i32, 1);
        assert_eq!(PrefetchLocality::NoTemporal as i32, 0);
        assert_eq!(PrefetchLocality::Low as i32, 1);
        assert_eq!(PrefetchLocality::Moderate as i32, 2);
        assert_eq!(PrefetchLocality::High as i32, 3);
    }
}
