//! Hardware performance counter abstraction.
//!
//! Provides [`HwCounters`] for reading grouped hardware counters
//! (instructions, branch misses, cache misses) independently of
//! the basic cycle/timing [`CycleCounter`](super::CycleCounter).
//!
//! [`NoopCounters`] is the fallback when hardware counters are
//! unavailable (wrong platform, missing permissions, feature disabled).

/// Deltas from hardware counters for a single measurement interval.
#[derive(Debug, Clone, Copy, Default)]
pub struct HwCounterDeltas {
    /// Instructions retired.
    pub instructions: u64,
    /// Branch misses.
    pub branch_misses: u64,
    /// L1D cache read misses.
    pub l1d_misses: u64,
    /// Last-level cache read misses.
    pub llc_misses: u64,
}

/// Trait for platform-specific grouped hardware counter access.
///
/// Implementations must be `Send + Sync` (same requirement as
/// [`CycleCounter`](super::CycleCounter)). All methods return
/// `Option` so callers handle unavailability uniformly.
pub trait HwCounters: Send + Sync {
    /// Opaque counter state captured at measurement start.
    type Snapshot: Send;

    /// Capture counter state. Returns `None` if counters unavailable.
    fn start(&self) -> Option<Self::Snapshot>;

    /// Read counter deltas since `snapshot`.
    ///
    /// Returns `None` if `snapshot` is `None` or reading fails.
    fn read(&self, snapshot: &Option<Self::Snapshot>) -> Option<HwCounterDeltas>;
}

/// No-op counters for platforms without hardware counter support.
pub struct NoopCounters;

impl NoopCounters {
    /// Create noop counters (always succeeds).
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoopCounters {
    fn default() -> Self {
        Self::new()
    }
}

impl HwCounters for NoopCounters {
    type Snapshot = ();

    fn start(&self) -> Option<Self::Snapshot> {
        None
    }

    fn read(&self, _snapshot: &Option<Self::Snapshot>) -> Option<HwCounterDeltas> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_counters_returns_none() {
        let c = NoopCounters;
        assert!(c.start().is_none());
        assert!(c.read(&None).is_none());
    }

    #[test]
    fn hw_counter_deltas_default_is_zero() {
        let d = HwCounterDeltas::default();
        assert_eq!(d.instructions, 0);
        assert_eq!(d.branch_misses, 0);
        assert_eq!(d.l1d_misses, 0);
        assert_eq!(d.llc_misses, 0);
    }
}
