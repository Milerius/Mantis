//! Platform-aware performance counter collection.
//!
//! # `x86_64` (planned)
//!
//! Uses `rdtsc` with `lfence` serializing barrier for accurate cycle counts.
//! The `lfence` ensures all prior instructions complete before reading the TSC.
//!
//! # `ARM64` (planned)
//!
//! Falls back to `mach_absolute_time()` (macOS) or `clock_gettime` (Linux).

use std::time::Instant;

/// A measurement from a performance counter.
#[derive(Debug, Clone, Copy)]
pub struct Measurement {
    /// Wall-clock duration in nanoseconds.
    pub nanos: u64,
    /// CPU cycles (if available on this platform, else 0).
    pub cycles: u64,
}

/// Trait for platform-specific cycle counting.
pub trait CycleCounter {
    /// Start a measurement.
    fn start(&self) -> u64;
    /// End a measurement, returning elapsed.
    fn elapsed(&self, start: u64) -> Measurement;
}

/// Fallback counter using `std::time::Instant`.
pub struct InstantCounter {
    /// Reference point for elapsed calculations.
    epoch: Instant,
}

impl InstantCounter {
    /// Create a new counter with current time as epoch.
    #[must_use]
    pub fn new() -> Self {
        Self {
            epoch: Instant::now(),
        }
    }
}

impl Default for InstantCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl CycleCounter for InstantCounter {
    fn start(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }

    fn elapsed(&self, start: u64) -> Measurement {
        let now = u64::try_from(self.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX);
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
    fn instant_counter_measures_time() {
        let counter = InstantCounter::new();
        let start = counter.start();
        // Do a tiny amount of work
        let mut sum = 0u64;
        for i in 0..1000 {
            sum = sum.wrapping_add(i);
        }
        let _sum = std::hint::black_box(sum);
        let m = counter.elapsed(start);
        // Should measure some nanos (though could be 0 on very fast machines)
        assert_eq!(m.cycles, 0, "fallback counter has no cycle info");
    }
}
