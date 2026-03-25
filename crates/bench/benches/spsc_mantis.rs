//! Criterion benchmarks for all Mantis SPSC ring presets.
//!
//! Uses `MantisMeasurement` with platform cycle counters instead of
//! criterion's default `WallTime`. After all benchmarks complete,
//! reads criterion's per-iteration estimates and combines them with
//! cycle counter data to produce a `BenchReport` JSON at
//! `target/bench-report-mantis.json`.

#![allow(missing_docs, clippy::print_stderr)]

use std::io::Write;

use criterion::{black_box, criterion_group, Criterion};
use mantis_bench::measurement::{
    read_criterion_estimates, reset_samples, take_samples, DefaultMeasurement,
};
use mantis_bench::report::{BenchReport, WorkloadResult};
use mantis_queue::SpscRing;

type MantisC = Criterion<DefaultMeasurement>;

fn mantis_criterion() -> MantisC {
    Criterion::default().with_measurement(DefaultMeasurement::platform_default())
}

/// Benchmark descriptor for post-run report generation.
struct BenchDesc {
    id: &'static str,
    element_type: &'static str,
    capacity: usize,
    mean_cycles_per_sample: Option<f64>,
}

fn bench_all(c: &mut MantisC) {
    let mut descs: Vec<BenchDesc> = Vec::new();

    // -- u64 workloads --
    descs.push(run_bench(c, "spsc/inline/single_item/u64", "u64", 1024, |b| {
        let mut ring = SpscRing::<u64, 1024>::new();
        b.iter(|| {
            let _ = ring.try_push(black_box(42u64));
            let _ = black_box(ring.try_pop());
        });
    }));

    descs.push(run_bench(c, "spsc/inline/burst_100/u64", "u64", 1024, |b| {
        let mut ring = SpscRing::<u64, 1024>::new();
        b.iter(|| {
            for i in 0..100u64 {
                let _ = ring.try_push(black_box(i));
            }
            for _ in 0..100 {
                let _ = black_box(ring.try_pop());
            }
        });
    }));

    descs.push(run_bench(c, "spsc/inline/burst_1000/u64", "u64", 2048, |b| {
        let mut ring = SpscRing::<u64, 2048>::new();
        b.iter(|| {
            for i in 0..1000u64 {
                let _ = ring.try_push(black_box(i));
            }
            for _ in 0..1000 {
                let _ = black_box(ring.try_pop());
            }
        });
    }));

    descs.push(run_bench(c, "spsc/inline/full_drain/u64", "u64", 1024, |b| {
        let mut ring = SpscRing::<u64, 1024>::new();
        b.iter(|| {
            for i in 0..1023u64 {
                let _ = ring.try_push(black_box(i));
            }
            while ring.try_pop().is_ok() {}
        });
    }));

    // -- Larger payloads --
    descs.push(run_bench(
        c,
        "spsc/inline/single_item/[u8;64]",
        "[u8; 64]",
        1024,
        |b| {
            let mut ring = SpscRing::<[u8; 64], 1024>::new();
            b.iter(|| {
                let _ = ring.try_push(black_box([0u8; 64]));
                let _ = black_box(ring.try_pop());
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/inline/single_item/[u8;256]",
        "[u8; 256]",
        256,
        |b| {
            let mut ring = SpscRing::<[u8; 256], 256>::new();
            b.iter(|| {
                let _ = ring.try_push(black_box([0u8; 256]));
                let _ = black_box(ring.try_pop());
            });
        },
    ));

    // -- Build and export report --
    export_report(&descs);
}

/// Run a single benchmark with our measurement, capture cycle samples.
fn run_bench(
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

/// Build a `BenchReport` by reading criterion's estimates and
/// combining with our cycle counter data.
fn export_report(descs: &[BenchDesc]) {
    let mut report = BenchReport::detect("SpscRing (inline)");
    "spsc".clone_into(&mut report.threading_model);
    if cfg!(feature = "asm") {
        report.features.push("asm".to_owned());
    }

    for desc in descs {
        // Read criterion's computed per-iteration statistics
        let estimates = read_criterion_estimates(desc.id);

        let mean_ns = estimates.map_or(0.0, |e| e.mean_ns);
        let median_ns = estimates.map_or(0.0, |e| e.median_ns);
        let std_dev = estimates.map_or(0.0, |e| e.std_dev_ns);

        // p50 ~ median, p99/p999 approximated from mean + std_dev
        // (criterion doesn't store percentiles directly; these are
        // Gaussian approximations — adequate for single-threaded benches)
        let p99_ns = mean_ns + 2.326 * std_dev;
        let p999_ns = mean_ns + 3.09 * std_dev;

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

    // Write report
    let base = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_owned());
    let dir = std::path::Path::new(&base)
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(std::path::Path::new("."));
    let path = dir.join("target").join("bench-report-mantis.json");

    match report.to_json() {
        Ok(json) => match std::fs::File::create(&path) {
            Ok(mut f) => {
                let _ = f.write_all(json.as_bytes());
                eprintln!("\n=== Mantis Benchmark Report ===");
                eprintln!("CPU:      {}", report.cpu);
                eprintln!("Arch:     {}", report.arch);
                eprintln!("Compiler: {}", report.compiler);
                eprintln!("Features: {:?}", report.features);
                eprintln!();
                for r in &report.results {
                    eprintln!(
                        "  {:40} {:>8.2} ns/op  {:>12.0} ops/s  p50={:.1}ns  p99={:.1}ns  cycles={:.0?}",
                        r.workload,
                        r.ns_per_op,
                        r.ops_per_sec,
                        r.p50_ns,
                        r.p99_ns,
                        r.cycles_per_op,
                    );
                }
                eprintln!("\nFull report: {}", path.display());
            }
            Err(e) => eprintln!("Failed to create report: {e}"),
        },
        Err(e) => eprintln!("Failed to serialize report: {e}"),
    }
}

criterion_group! {
    name = benches;
    config = mantis_criterion();
    targets = bench_all
}

criterion::criterion_main!(benches);
