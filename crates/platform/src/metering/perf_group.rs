#![expect(
    unsafe_code,
    reason = "UnsafeCell + Sync impl for single-threaded perf-event2 Group access"
)]
//! Linux `perf_event_open` grouped hardware counters.
//!
//! Uses the [`perf-event2`] crate to read instructions retired, branch misses,
//! L1D cache read misses, and LLC read misses as an atomic group.
//!
//! Initialization fails gracefully if `perf_event_open` is denied
//! (e.g., `perf_event_paranoid >= 2` on CI runners).

use std::cell::UnsafeCell;
use std::io;

use perf_event::events::{Cache, CacheId, CacheOp, CacheResult, Hardware};
use perf_event::{Builder, Counter, Group};

use super::hw_counters::{HwCounterDeltas, HwCounters};

/// L1D read miss cache event.
const L1D_READ_MISS: Cache = Cache {
    which: CacheId::L1D,
    operation: CacheOp::READ,
    result: CacheResult::MISS,
};

/// Last-level cache read miss event.
const LLC_READ_MISS: Cache = Cache {
    which: CacheId::LL,
    operation: CacheOp::READ,
    result: CacheResult::MISS,
};

/// Grouped hardware counters via Linux `perf_event_open`.
///
/// Reads 4 counters atomically: instructions, branch misses,
/// L1D read misses, LLC read misses. The group is enabled/disabled
/// around each measurement interval.
///
/// `Group` requires `&mut self` for `enable`/`disable`/`read`, but
/// [`HwCounters`] takes `&self`. We use [`UnsafeCell`] for interior
/// mutability — this is sound because criterion benchmarks are
/// single-threaded (one bench thread calls start/read sequentially).
pub struct PerfGroupCounters {
    group: UnsafeCell<Group>,
    instructions: Counter,
    branch_misses: Counter,
    l1d_misses: Counter,
    llc_misses: Counter,
}

// SAFETY: PerfGroupCounters is only accessed from a single benchmark
// thread at a time. Criterion's bench_function runs the closure on one
// thread, calling start() then read() sequentially. The Sync bound is
// required by HwCounters: Send + Sync but no concurrent access occurs.
unsafe impl Sync for PerfGroupCounters {}

/// Snapshot of counter values at measurement start.
pub struct PerfSnapshot {
    instructions: u64,
    branch_misses: u64,
    l1d_misses: u64,
    llc_misses: u64,
}

impl PerfGroupCounters {
    /// Try to create a grouped perf counter set.
    ///
    /// # Errors
    ///
    /// Returns `io::Error` if `perf_event_open` fails (permissions,
    /// unsupported events, or `perf_event_paranoid >= 2`).
    pub fn try_new() -> Result<Self, io::Error> {
        let mut group = Group::new()?;
        let instructions = group.add(&Builder::new(Hardware::INSTRUCTIONS))?;
        let branch_misses = group.add(&Builder::new(Hardware::BRANCH_MISSES))?;
        let l1d_misses = group.add(&Builder::new(L1D_READ_MISS))?;
        let llc_misses = group.add(&Builder::new(LLC_READ_MISS))?;
        Ok(Self {
            group: UnsafeCell::new(group),
            instructions,
            branch_misses,
            l1d_misses,
            llc_misses,
        })
    }

}

impl HwCounters for PerfGroupCounters {
    type Snapshot = PerfSnapshot;

    fn start(&self) -> Option<Self::Snapshot> {
        // SAFETY: single-threaded access during benchmark measurement.
        // Criterion calls start() then read() sequentially on one thread.
        // UnsafeCell grants interior mutability; no concurrent access occurs.
        let group = unsafe { &mut *self.group.get() };
        group.enable().ok()?;
        let counts = group.read().ok()?;
        Some(PerfSnapshot {
            instructions: counts[&self.instructions],
            branch_misses: counts[&self.branch_misses],
            l1d_misses: counts[&self.l1d_misses],
            llc_misses: counts[&self.llc_misses],
        })
    }

    fn read(&self, snapshot: &Option<Self::Snapshot>) -> Option<HwCounterDeltas> {
        let base = snapshot.as_ref()?;
        // SAFETY: single-threaded access, same as start().
        let group = unsafe { &mut *self.group.get() };
        let counts = group.read().ok()?;
        group.disable().ok()?;
        Some(HwCounterDeltas {
            instructions: counts[&self.instructions].saturating_sub(base.instructions),
            branch_misses: counts[&self.branch_misses].saturating_sub(base.branch_misses),
            l1d_misses: counts[&self.l1d_misses].saturating_sub(base.l1d_misses),
            llc_misses: counts[&self.llc_misses].saturating_sub(base.llc_misses),
        })
    }
}
