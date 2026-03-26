//! Criterion `Measurement` implementation backed by platform counters.
//!
//! `MantisMeasurement<C>` wraps a [`CycleCounter`] so that criterion
//! benchmarks automatically collect cycle data alongside wall time.
//! Cycle samples are stored in a thread-local [`SampleCollector`] for
//! later retrieval and inclusion in [`BenchReport`](crate::report::BenchReport).

use std::cell::RefCell;
use std::time::Duration;

use criterion::measurement::{Measurement, ValueFormatter, WallTime};

use mantis_platform::metering::{
    CycleCounter, DefaultCounter, DefaultHwCounters, HwCounters,
};

/// Collects cycle measurements from benchmark iterations.
///
/// Criterion calls `start()`/`end()` once per *sample*, where each
/// sample contains many iterations. We store the raw cycle+nanos per
/// sample. Use `mean_cycles_per_sample()` for raw data, or combine
/// with criterion's iteration count for per-op metrics.
#[derive(Debug, Default)]
pub struct SampleCollector {
    /// Cycle counts from each sample.
    pub cycles: Vec<u64>,
    /// Wall-time nanoseconds from each sample.
    pub nanos: Vec<u64>,
    /// Instructions retired per sample (empty if hw counters unavailable).
    pub instructions: Vec<u64>,
    /// Branch misses per sample.
    pub branch_misses: Vec<u64>,
    /// L1D cache read misses per sample.
    pub l1d_misses: Vec<u64>,
    /// LLC read misses per sample.
    pub llc_misses: Vec<u64>,
    /// Whether hardware counters were collected for these samples.
    pub has_hw_counters: bool,
}

impl SampleCollector {
    /// Reset the collector for a new benchmark.
    pub fn reset(&mut self) {
        self.cycles.clear();
        self.nanos.clear();
        self.instructions.clear();
        self.branch_misses.clear();
        self.l1d_misses.clear();
        self.llc_misses.clear();
        self.has_hw_counters = false;
    }

    /// Number of samples collected.
    #[must_use]
    pub fn len(&self) -> usize {
        self.cycles.len()
    }

    /// Whether the collector is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cycles.is_empty()
    }

    /// Mean cycles per sample (not per iteration).
    #[must_use]
    #[expect(clippy::cast_precision_loss, reason = "sample counts fit f64")]
    pub fn mean_cycles_per_sample(&self) -> Option<f64> {
        if self.cycles.is_empty() {
            return None;
        }
        let sum: u64 = self.cycles.iter().sum();
        Some(sum as f64 / self.cycles.len() as f64)
    }

    /// Mean of a hw counter vector per sample, or `None` if hw counters inactive.
    #[expect(clippy::cast_precision_loss, reason = "sample counts fit f64")]
    fn mean_hw_counter(&self, values: &[u64]) -> Option<f64> {
        if !self.has_hw_counters || values.is_empty() {
            return None;
        }
        let sum: u64 = values.iter().sum();
        Some(sum as f64 / values.len() as f64)
    }

    /// Mean instructions per sample.
    #[must_use]
    pub fn mean_instructions_per_sample(&self) -> Option<f64> {
        self.mean_hw_counter(&self.instructions)
    }

    /// Mean branch misses per sample.
    #[must_use]
    pub fn mean_branch_misses_per_sample(&self) -> Option<f64> {
        self.mean_hw_counter(&self.branch_misses)
    }

    /// Mean L1D cache misses per sample.
    #[must_use]
    pub fn mean_l1d_misses_per_sample(&self) -> Option<f64> {
        self.mean_hw_counter(&self.l1d_misses)
    }

    /// Mean LLC misses per sample.
    #[must_use]
    pub fn mean_llc_misses_per_sample(&self) -> Option<f64> {
        self.mean_hw_counter(&self.llc_misses)
    }
}

thread_local! {
    /// Thread-local sample collector for the current benchmark.
    static SAMPLES: RefCell<SampleCollector> = RefCell::new(SampleCollector::default());
}

/// Reset the thread-local sample collector.
pub fn reset_samples() {
    SAMPLES.with(|s| s.borrow_mut().reset());
}

/// Take the collected samples, leaving the collector empty.
#[must_use]
pub fn take_samples() -> SampleCollector {
    SAMPLES.with(|s| std::mem::take(&mut *s.borrow_mut()))
}

/// Criterion measurement backed by a platform cycle counter.
///
/// Wraps a [`CycleCounter`] and delegates formatting to [`WallTime`].
/// Optionally collects hardware counters ([`DefaultHwCounters`]) when
/// the `perf-counters` feature is enabled and the platform supports it.
///
/// On each `end()` call, stores cycle + nanos + hw counter deltas in
/// the thread-local [`SampleCollector`] for later retrieval.
pub struct MantisMeasurement<C: CycleCounter> {
    counter: C,
    wall: WallTime,
    hw: Option<DefaultHwCounters>,
}

/// Hw counter snapshot type alias for readability.
type HwSnapshot = <DefaultHwCounters as HwCounters>::Snapshot;

impl<C: CycleCounter> MantisMeasurement<C> {
    /// Create a new measurement with the given counter (no hw counters).
    pub fn new(counter: C) -> Self {
        Self {
            counter,
            wall: WallTime,
            hw: None,
        }
    }

    /// Create a measurement with both cycle counter and hw counters.
    pub fn with_hw_counters(counter: C, hw: DefaultHwCounters) -> Self {
        Self {
            counter,
            wall: WallTime,
            hw: Some(hw),
        }
    }
}

impl<C: CycleCounter> Measurement for MantisMeasurement<C> {
    type Intermediate = (u64, std::time::Instant, Option<HwSnapshot>);
    type Value = Duration;

    fn start(&self) -> Self::Intermediate {
        let hw_snap = self.hw.as_ref().and_then(HwCounters::start);
        let cycles = self.counter.start();
        let wall_start = std::time::Instant::now();
        (cycles, wall_start, hw_snap)
    }

    fn end(&self, i: Self::Intermediate) -> Self::Value {
        let wall_elapsed = i.1.elapsed();
        let m = self.counter.elapsed(i.0);
        let hw_deltas = self.hw.as_ref().and_then(|hw| hw.read(&i.2));

        // Store the sample for later retrieval
        SAMPLES.with(|s| {
            let mut collector = s.borrow_mut();
            collector.cycles.push(m.cycles);
            collector.nanos.push(m.nanos);
            if let Some(d) = hw_deltas {
                collector.instructions.push(d.instructions);
                collector.branch_misses.push(d.branch_misses);
                collector.l1d_misses.push(d.l1d_misses);
                collector.llc_misses.push(d.llc_misses);
                collector.has_hw_counters = true;
            }
        });

        wall_elapsed
    }

    fn add(&self, v1: &Self::Value, v2: &Self::Value) -> Self::Value {
        *v1 + *v2
    }

    fn zero(&self) -> Self::Value {
        Duration::ZERO
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "nanosecond counts fit in f64 for typical benchmark durations"
    )]
    fn to_f64(&self, value: &Self::Value) -> f64 {
        value.as_nanos() as f64
    }

    fn formatter(&self) -> &dyn ValueFormatter {
        self.wall.formatter()
    }
}

/// Default measurement using the platform-selected counter.
pub type DefaultMeasurement = MantisMeasurement<DefaultCounter>;

impl DefaultMeasurement {
    /// Create a default measurement for the current platform.
    ///
    /// Attempts to initialize hardware counters if the `perf-counters`
    /// feature is enabled. Falls back gracefully to cycles-only if
    /// hw counter initialization fails.
    #[must_use]
    pub fn platform_default() -> Self {
        Self::new(DefaultCounter::default())
    }
}

/// Read criterion's estimates.json for a benchmark and extract
/// the per-iteration nanosecond statistics.
///
/// Returns `(mean_ns, median_ns)` or `None` if the file doesn't exist.
#[must_use]
pub fn read_criterion_estimates(bench_id: &str) -> Option<CriterionEstimates> {
    // Criterion saves with / replaced by _
    let dir_name = bench_id.replace('/', "_");
    // Criterion writes to the workspace target/ dir, which may differ
    // from the binary's cwd. Use CARGO_MANIFEST_DIR to find it.
    let base = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_owned());
    let workspace = std::path::Path::new(&base)
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(std::path::Path::new("."));
    let path = workspace
        .join("target")
        .join("criterion")
        .join(&dir_name)
        .join("new")
        .join("estimates.json");
    let content = std::fs::read_to_string(&path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&content).ok()?;

    Some(CriterionEstimates {
        mean_ns: v["mean"]["point_estimate"].as_f64()?,
        median_ns: v["median"]["point_estimate"].as_f64()?,
        std_dev_ns: v["std_dev"]["point_estimate"].as_f64().unwrap_or(0.0),
    })
}

/// Parsed criterion estimates for a benchmark.
#[derive(Debug, Clone, Copy)]
pub struct CriterionEstimates {
    /// Mean nanoseconds per iteration.
    pub mean_ns: f64,
    /// Median nanoseconds per iteration.
    pub median_ns: f64,
    /// Standard deviation of nanoseconds per iteration.
    pub std_dev_ns: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mantis_platform::metering::InstantCounter;

    #[test]
    fn measurement_creation() {
        let m = MantisMeasurement::new(InstantCounter::new());
        let _start = Measurement::start(&m);
    }

    #[test]
    fn measurement_captures_samples() {
        let m = MantisMeasurement::new(InstantCounter::new());
        reset_samples();

        let i = Measurement::start(&m);
        std::hint::black_box(42);
        let _d = Measurement::end(&m, i);

        let samples = take_samples();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples.nanos.len(), 1);
    }

    #[test]
    fn sample_collector_mean() {
        let mut collector = SampleCollector::default();
        collector.cycles.extend_from_slice(&[10, 20, 30]);
        assert!((collector.mean_cycles_per_sample().unwrap_or(0.0) - 20.0).abs() < 0.01);
    }
}
