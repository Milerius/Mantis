//! Fallback counter using [`std::time::Instant`].

use std::time::Instant;

use super::{CycleCounter, Measurement};

/// `Instant`-based counter for platforms without hardware cycle access.
///
/// Reports nanoseconds via `Instant::elapsed`; `cycles` is always 0.
pub struct InstantCounter {
    epoch: Instant,
}

impl InstantCounter {
    /// Create a new counter with the current time as epoch.
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
            instructions: None,
            branch_misses: None,
            l1d_misses: None,
            llc_misses: None,
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
        let mut sum = 0u64;
        for i in 0..1000u64 {
            sum = sum.wrapping_add(i);
        }
        let _sum = std::hint::black_box(sum);
        let m = counter.elapsed(start);
        assert_eq!(m.cycles, 0, "fallback counter has no cycle info");
    }

    #[test]
    fn instant_counter_default() {
        let counter = InstantCounter::default();
        let start = counter.start();
        let m = counter.elapsed(start);
        assert_eq!(m.cycles, 0);
        // nanos should be a small non-negative number (saturating_sub guarantees >= 0)
        let _ = m.nanos;
    }
}
