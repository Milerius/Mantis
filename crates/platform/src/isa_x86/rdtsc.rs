#![expect(unsafe_code, reason = "RDTSC requires inline asm")]
//! RDTSC-based cycle counter for `x86_64`.

use std::time::Instant;

use crate::metering::{CycleCounter, Measurement};

#[inline]
fn rdtsc_serialized() -> u64 {
    // SAFETY: `lfence` serializes prior loads before the `rdtsc` instruction,
    // ensuring we read a stable TSC value. The trailing `lfence` prevents
    // out-of-order execution from moving subsequent instructions before the
    // read. We only clobber `eax`/`edx` which are declared as outputs.
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

/// Cycle counter using the `RDTSC` instruction with `lfence` serialization.
pub struct RdtscCounter {
    epoch: Instant,
    epoch_tsc: u64,
}

impl RdtscCounter {
    /// Create a new counter, capturing the current wall-clock time and TSC
    /// value as the epoch for nanosecond conversion.
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
        let t0 = counter.start();
        // A small amount of work to ensure the TSC advances.
        let _ = (0u64..1000).fold(0u64, |acc, x| acc.wrapping_add(x));
        let t1 = counter.start();
        assert!(t1 >= t0, "TSC must be monotonically non-decreasing");
    }
}
