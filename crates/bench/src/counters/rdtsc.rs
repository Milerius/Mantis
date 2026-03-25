// SAFETY: This file contains inline assembly for the x86_64 RDTSC instruction.
// All unsafe blocks are isolated here; the crate root and workspace deny unsafe_code,
// so this file requires an explicit allow.
#![allow(unsafe_code)]

//! RDTSC-based cycle counter for `x86_64`.
//!
//! Uses `lfence; rdtsc; lfence` to serialize the timestamp read, preventing
//! out-of-order execution from skewing measurements.

use std::time::Instant;

use crate::counters::{CycleCounter, Measurement};

/// Reads the TSC with serializing barriers on both sides.
///
/// # Safety
///
/// Valid only on `x86_64`. Caller must ensure the CPU supports `rdtsc`
/// (all modern `x86_64` CPUs do). The `lfence` barriers prevent reordering.
#[inline]
fn rdtsc_serialized() -> u64 {
    // SAFETY: `rdtsc` is available on all x86_64 CPUs. `lfence` barriers
    // prevent instruction reordering across the counter read. The two output
    // halves (edx = high 32 bits, eax = low 32 bits) are combined into a u64.
    // No memory is accessed; only registers are touched.
    unsafe {
        let lo: u32;
        let hi: u32;
        core::arch::asm!(
            "lfence",
            "rdtsc",
            "lfence",
            out("eax") lo,
            out("edx") hi,
            options(nostack, nomem),
        );
        (u64::from(hi) << 32) | u64::from(lo)
    }
}

/// Cycle counter using `rdtsc` with `lfence` serialization.
///
/// Reports raw TSC ticks in `cycles`; `nanos` is derived from `Instant` elapsed
/// since construction and is a coarse approximation only.
pub struct RdtscCounter {
    /// Wall-clock epoch for fallback nanosecond estimation.
    epoch: Instant,
    /// TSC value at construction.
    epoch_tsc: u64,
}

impl RdtscCounter {
    /// Create a new counter, recording both wall-clock and TSC epoch.
    #[must_use]
    pub fn new() -> Self {
        Self {
            epoch: Instant::now(),
            epoch_tsc: rdtsc_serialized(),
        }
    }
}

impl Default for RdtscCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl CycleCounter for RdtscCounter {
    fn start(&self) -> u64 {
        rdtsc_serialized()
    }

    fn elapsed(&self, start: u64) -> Measurement {
        let end = rdtsc_serialized();
        let cycles = end.saturating_sub(start);
        let nanos = u64::try_from(self.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX);
        let epoch_cycles = rdtsc_serialized().saturating_sub(self.epoch_tsc);
        // Approximate nanos for the measured interval by linear interpolation.
        let interval_nanos = nanos
            .saturating_mul(cycles)
            .checked_div(epoch_cycles)
            .unwrap_or(0);
        Measurement {
            nanos: interval_nanos,
            cycles,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rdtsc_counter_increases() {
        let counter = RdtscCounter::new();
        let start = counter.start();
        let mut sum = 0u64;
        for i in 0..1000 {
            sum = sum.wrapping_add(i);
        }
        let _sum = std::hint::black_box(sum);
        let m = counter.elapsed(start);
        assert!(m.cycles > 0, "TSC must advance");
    }
}
