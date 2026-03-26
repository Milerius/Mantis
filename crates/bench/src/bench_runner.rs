//! Shared benchmark runner infrastructure for criterion benchmarks.
//!
//! Provides the common `BenchDesc`, `run_bench`, and `export_report`
//! used by both Mantis and contender benchmark binaries.

#![expect(clippy::print_stderr, reason = "benchmark runner reports to stderr")]

use std::io::Write;

use criterion::Criterion;

use crate::measurement::{
    DefaultMeasurement, read_criterion_estimates, reset_samples, take_samples,
};
use crate::report::{BenchReport, WorkloadResult};

/// Criterion configured with our platform cycle counter.
pub type MantisC = Criterion<DefaultMeasurement>;

/// Create a criterion instance using our custom measurement.
#[must_use]
pub fn mantis_criterion() -> MantisC {
    Criterion::default().with_measurement(DefaultMeasurement::platform_default())
}

/// Descriptor collected after each benchmark run.
pub struct BenchDesc {
    /// Benchmark ID (e.g., `"spsc/inline/single_item/u64"`).
    pub id: &'static str,
    /// Element type string for the report.
    pub element_type: &'static str,
    /// Ring capacity used.
    pub capacity: usize,
    /// Mean cycles per sample from our counter.
    pub mean_cycles_per_sample: Option<f64>,
}

/// Run a single benchmark, capturing cycle samples alongside criterion.
pub fn run_bench(
    c: &mut MantisC,
    id: &'static str,
    element_type: &'static str,
    capacity: usize,
    f: impl Fn(&mut criterion::Bencher<'_, DefaultMeasurement>),
) -> BenchDesc {
    reset_samples();
    c.bench_function(id, |b| f(b));
    let samples = take_samples();
    BenchDesc {
        id,
        element_type,
        capacity,
        mean_cycles_per_sample: samples.mean_cycles_per_sample(),
    }
}

/// Build a `BenchReport` from descriptors and write JSON to disk.
///
/// Reads criterion's per-iteration estimates and combines them with
/// our cycle counter data. Writes to
/// `target/bench-report-{filename}.json`.
#[expect(clippy::print_stderr, reason = "benchmark report output")]
pub fn export_report(
    descs: &[BenchDesc],
    implementation: &str,
    filename: &str,
    features: Vec<String>,
) {
    // Gaussian z-score approximations (criterion doesn't store
    // raw percentiles; adequate for single-threaded benches).
    const Z_99: f64 = 2.326;
    const Z_999: f64 = 3.090;

    let mut report = BenchReport::detect(implementation);
    "spsc".clone_into(&mut report.threading_model);
    report.features = features;

    for desc in descs {
        let estimates = read_criterion_estimates(desc.id);
        let mean_ns = estimates.map_or(0.0, |e| e.mean_ns);
        let median_ns = estimates.map_or(0.0, |e| e.median_ns);
        let std_dev = estimates.map_or(0.0, |e| e.std_dev_ns);
        let p99_ns = mean_ns + Z_99 * std_dev;
        let p999_ns = mean_ns + Z_999 * std_dev;
        let ops_per_sec = if mean_ns > 0.0 {
            1_000_000_000.0 / mean_ns
        } else {
            0.0
        };

        report.results.push(WorkloadResult {
            workload: desc.id.to_owned(),
            element_type: desc.element_type.to_owned(),
            capacity: desc.capacity,
            ops_per_sec,
            ns_per_op: mean_ns,
            p50_ns: median_ns,
            p99_ns,
            p999_ns,
            cycles_per_op: desc.mean_cycles_per_sample,
            instructions_per_op: None,
            branch_misses_per_op: None,
            l1_misses_per_op: None,
            llc_misses_per_op: None,
            full_rate: None,
            empty_rate: None,
            mean_occupancy: None,
        });
    }

    let base = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_owned());
    let dir = std::path::Path::new(&base)
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(std::path::Path::new("."));
    let path = dir
        .join("target")
        .join(format!("bench-report-{filename}.json"));

    match report.to_json() {
        Ok(json) => match std::fs::File::create(&path) {
            Ok(mut f) => {
                let _ = f.write_all(json.as_bytes());
                eprintln!("\n=== {implementation} Benchmark Report ===");
                eprintln!("CPU:      {}", report.cpu);
                eprintln!("Arch:     {}", report.arch);
                eprintln!("Compiler: {}", report.compiler);
                if !report.features.is_empty() {
                    eprintln!("Features: {:?}", report.features);
                }
                eprintln!();
                for r in &report.results {
                    eprintln!(
                        "  {:40} {:>8.2} ns/op  {:>12.0} ops/s  \
                         p50={:.1}ns  p99={:.1}ns  cycles={:.0?}",
                        r.workload, r.ns_per_op, r.ops_per_sec, r.p50_ns, r.p99_ns, r.cycles_per_op,
                    );
                }
                eprintln!("\nFull report: {}", path.display());
            }
            Err(e) => eprintln!("Failed to create report: {e}"),
        },
        Err(e) => eprintln!("Failed to serialize report: {e}"),
    }
}
