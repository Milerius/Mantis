//! Criterion benchmarks for all Mantis SPSC ring presets.
//!
//! Uses `MantisMeasurement` with platform cycle counters instead of
//! criterion's default `WallTime`. After all benchmarks complete,
//! reads criterion's per-iteration estimates and combines them with
//! cycle counter data to produce a `BenchReport` JSON at
//! `target/bench-report-mantis.json`.

#![allow(missing_docs, clippy::print_stderr)]

use criterion::{black_box, criterion_group};
use mantis_bench::bench_runner::{
    export_report, mantis_criterion, run_bench, BenchDesc, MantisC,
};
use mantis_queue::SpscRing;

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
    let mut features = Vec::new();
    if cfg!(feature = "asm") {
        features.push("asm".to_owned());
    }
    export_report(&descs, "SpscRing (inline)", "mantis", features);
}

criterion_group! {
    name = benches;
    config = mantis_criterion();
    targets = bench_all
}

criterion::criterion_main!(benches);
