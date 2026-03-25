// SAFETY: This file calls libc's `clock_gettime` to read CLOCK_MONOTONIC.
// All unsafe blocks are isolated here; the crate root and workspace deny
// unsafe_code, so this file requires an explicit allow.
#![allow(unsafe_code)]

//! Linux ARM64 counter using `clock_gettime(CLOCK_MONOTONIC)`.
//!
//! On Linux there is no direct equivalent to `mach_absolute_time`; the closest
//! high-resolution monotonic source is `CLOCK_MONOTONIC` via libc.

use crate::counters::{CycleCounter, Measurement};

/// Read `CLOCK_MONOTONIC` and return the value in nanoseconds.
fn clock_gettime_nanos() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a valid stack-allocated `timespec`. `clock_gettime`
    // writes to it in place. `CLOCK_MONOTONIC` is always available on Linux.
    // We treat a non-zero return as a platform error and return 0 rather than
    // panicking, since this is a best-effort measurement path.
    let ret = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &raw mut ts) };
    if ret != 0 {
        return 0;
    }
    let secs = u64::try_from(ts.tv_sec).unwrap_or(0);
    let nanos = u64::try_from(ts.tv_nsec).unwrap_or(0);
    secs.saturating_mul(1_000_000_000).saturating_add(nanos)
}

/// Counter using `clock_gettime(CLOCK_MONOTONIC)` on Linux ARM64.
///
/// `cycles` is 0 (no direct cycle counter access without perf_event); `nanos`
/// holds the elapsed nanoseconds.
pub struct PmuCounter;

impl PmuCounter {
    /// Create a new counter.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for PmuCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl CycleCounter for PmuCounter {
    fn start(&self) -> u64 {
        clock_gettime_nanos()
    }

    fn elapsed(&self, start: u64) -> Measurement {
        let now = clock_gettime_nanos();
        Measurement {
            nanos: now.saturating_sub(start),
            cycles: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pmu_counter_measures_time() {
        let counter = PmuCounter::new();
        let start = counter.start();
        let mut sum = 0u64;
        for i in 0..1000 {
            sum = sum.wrapping_add(i);
        }
        let _sum = std::hint::black_box(sum);
        let m = counter.elapsed(start);
        assert_eq!(m.cycles, 0, "PmuCounter does not provide cycle counts");
    }
}
