//! Shared benchmark runner infrastructure for criterion benchmarks.
//!
//! Provides the common `BenchDesc`, `run_bench`, and `export_report`
//! used by both Mantis and contender benchmark binaries.

use std::io::Write;

use criterion::Criterion;

use crate::measurement::{
    DefaultMeasurement, read_criterion_estimates, read_criterion_sample_iters,
    reset_samples, take_samples,
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
    /// Mean cycles per operation.
    pub cycles_per_op: Option<f64>,
    /// Mean instructions per operation (hw counters).
    pub instructions_per_op: Option<f64>,
    /// Mean branch misses per operation (hw counters).
    pub branch_misses_per_op: Option<f64>,
    /// Mean L1D cache misses per operation (hw counters).
    pub l1d_misses_per_op: Option<f64>,
    /// Mean LLC misses per operation (hw counters).
    pub llc_misses_per_op: Option<f64>,
}

/// Run a single benchmark, capturing cycle samples alongside criterion.
///
/// After criterion finishes, reads the per-sample iteration counts from
/// `sample.json` and normalizes all counter values to per-operation.
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
    let iters = read_criterion_sample_iters(id).unwrap_or_default();
    BenchDesc {
        id,
        element_type,
        capacity,
        cycles_per_op: samples.mean_cycles_per_op(&iters),
        instructions_per_op: samples.mean_instructions_per_op(&iters),
        branch_misses_per_op: samples.mean_branch_misses_per_op(&iters),
        l1d_misses_per_op: samples.mean_l1d_misses_per_op(&iters),
        llc_misses_per_op: samples.mean_llc_misses_per_op(&iters),
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
            cycles_per_op: desc.cycles_per_op,
            instructions_per_op: desc.instructions_per_op,
            branch_misses_per_op: desc.branch_misses_per_op,
            l1_misses_per_op: desc.l1d_misses_per_op,
            llc_misses_per_op: desc.llc_misses_per_op,
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
                    eprint!(
                        "  {:40} {:>8.2} ns/op  {:>12.0} ops/s  \
                         p50={:.1}ns  p99={:.1}ns  cycles={:.0?}",
                        r.workload, r.ns_per_op, r.ops_per_sec,
                        r.p50_ns, r.p99_ns, r.cycles_per_op,
                    );
                    if let Some(insns) = r.instructions_per_op {
                        eprint!("  insns={insns:.0}");
                    }
                    if let Some(bm) = r.branch_misses_per_op {
                        eprint!("  bmiss={bm:.1}");
                    }
                    if let Some(l1) = r.l1_misses_per_op {
                        eprint!("  l1d={l1:.1}");
                    }
                    if let Some(llc) = r.llc_misses_per_op {
                        eprint!("  llc={llc:.1}");
                    }
                    eprintln!();
                }
                eprintln!("\nFull report: {}", path.display());
            }
            Err(e) => eprintln!("Failed to create report: {e}"),
        },
        Err(e) => eprintln!("Failed to serialize report: {e}"),
    }
}
