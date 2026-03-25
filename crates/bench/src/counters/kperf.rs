// SAFETY: This file calls macOS system APIs (`mach_absolute_time`,
// `mach_timebase_info`) via extern "C". All unsafe blocks are isolated here;
// the crate root and workspace deny unsafe_code, so this file requires an
// explicit allow.
#![allow(unsafe_code)]

//! macOS ARM64 cycle counter using `mach_absolute_time`.
//!
//! `mach_absolute_time()` returns CPU-frequency-relative ticks. Dividing by
//! the timebase (numer/denom) converts to nanoseconds. On Apple Silicon the
//! timebase is typically 1/1, making ticks == nanoseconds.

use crate::counters::{CycleCounter, Measurement};

/// Mirror of the C `mach_timebase_info_data_t` struct.
#[repr(C)]
struct MachTimebaseInfo {
    numer: u32,
    denom: u32,
}

// SAFETY: Both functions are well-known stable macOS system calls available
// on all macOS versions Mantis targets. `mach_absolute_time` reads a hardware
// register and writes nothing. `mach_timebase_info` writes only to the
// caller-supplied pointer, which callers guarantee is valid.
unsafe extern "C" {
    /// Returns the current value of the Mach absolute time clock.
    fn mach_absolute_time() -> u64;

    /// Fills `info` with the conversion factors for `mach_absolute_time` ticks.
    fn mach_timebase_info(info: *mut MachTimebaseInfo) -> i32;
}

/// Reads the current `mach_absolute_time` value.
#[inline]
fn read_mach_time() -> u64 {
    // SAFETY: `mach_absolute_time` takes no arguments, writes no memory, and
    // is always available on macOS. It is async-signal-safe per Apple docs.
    unsafe { mach_absolute_time() }
}

/// Fetches the timebase conversion factors.
///
/// Returns `(numer, denom)`. On Apple Silicon this is typically `(1, 1)`.
fn fetch_timebase() -> (u32, u32) {
    let mut info = MachTimebaseInfo { numer: 1, denom: 1 };
    // SAFETY: `info` is a valid stack-allocated MachTimebaseInfo with the
    // correct repr(C) layout. `mach_timebase_info` fills it in place.
    // We ignore the return value; on failure the struct retains safe defaults.
    unsafe {
        mach_timebase_info(core::ptr::addr_of_mut!(info));
    }
    (info.numer, info.denom)
}

/// Counter using `mach_absolute_time` for macOS ARM64.
///
/// `cycles` holds raw Mach ticks; `nanos` is derived via the timebase ratio.
pub struct KperfCounter {
    /// Numerator of the Mach timebase ratio.
    numer: u64,
    /// Denominator of the Mach timebase ratio.
    denom: u64,
}

impl KperfCounter {
    /// Create a new counter, fetching the timebase conversion once.
    #[must_use]
    pub fn new() -> Self {
        let (numer, denom) = fetch_timebase();
        Self {
            numer: u64::from(numer),
            denom: u64::from(denom).max(1),
        }
    }

    /// Convert Mach ticks to nanoseconds using the stored timebase ratio.
    fn ticks_to_nanos(&self, ticks: u64) -> u64 {
        ticks.saturating_mul(self.numer) / self.denom
    }
}

impl Default for KperfCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl CycleCounter for KperfCounter {
    fn start(&self) -> u64 {
        read_mach_time()
    }

    fn elapsed(&self, start: u64) -> Measurement {
        let end = read_mach_time();
        let ticks = end.saturating_sub(start);
        Measurement {
            nanos: self.ticks_to_nanos(ticks),
            cycles: ticks,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kperf_counter_advances() {
        let counter = KperfCounter::new();
        let start = counter.start();
        let mut sum = 0u64;
        for i in 0..1000 {
            sum = sum.wrapping_add(i);
        }
        let _sum = std::hint::black_box(sum);
        let m = counter.elapsed(start);
        assert!(m.cycles > 0, "mach_absolute_time must advance");
        assert!(m.nanos > 0, "nanoseconds must be non-zero after conversion");
    }

    #[test]
    fn kperf_timebase_is_valid() {
        let counter = KperfCounter::new();
        assert!(counter.denom > 0, "denom must be non-zero");
        assert!(counter.numer > 0, "numer must be non-zero");
    }
}
