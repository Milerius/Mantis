//! Criterion benchmarks for all Mantis SPSC ring presets.
//!
//! Uses `MantisMeasurement` with platform cycle counters instead of
//! criterion's default `WallTime`. After all benchmarks complete,
//! reads criterion's per-iteration estimates and combines them with
//! cycle counter data to produce a `BenchReport` JSON at
//! `target/bench-report-mantis.json`.

#![allow(missing_docs, clippy::print_stderr)]

use criterion::{black_box, criterion_group};
use mantis_bench::bench_runner::{BenchDesc, MantisC, export_report, mantis_criterion, run_bench};
use mantis_bench::messages::{Message48, Message64, make_msg48, make_msg64};
use mantis_bench::workloads::{batch_copy, burst_copy};
use mantis_queue::{SpscRing, SpscRingCopy};

fn bench_all(c: &mut MantisC) {
    let mut descs: Vec<BenchDesc> = Vec::new();

    // -- u64 workloads --
    descs.push(run_bench(
        c,
        "spsc/inline/single_item/u64",
        "u64",
        1024,
        |b| {
            let mut ring = SpscRing::<u64, 1024>::new();
            b.iter(|| {
                let _ = ring.try_push(black_box(42u64));
                let _ = black_box(ring.try_pop());
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/inline/burst_100/u64",
        "u64",
        1024,
        |b| {
            let mut ring = SpscRing::<u64, 1024>::new();
            b.iter(|| {
                for i in 0..100u64 {
                    let _ = ring.try_push(black_box(i));
                }
                for _ in 0..100 {
                    let _ = black_box(ring.try_pop());
                }
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/inline/burst_1000/u64",
        "u64",
        2048,
        |b| {
            let mut ring = SpscRing::<u64, 2048>::new();
            b.iter(|| {
                for i in 0..1000u64 {
                    let _ = ring.try_push(black_box(i));
                }
                for _ in 0..1000 {
                    let _ = black_box(ring.try_pop());
                }
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/inline/full_drain/u64",
        "u64",
        1024,
        |b| {
            let mut ring = SpscRing::<u64, 1024>::new();
            b.iter(|| {
                for i in 0..1023u64 {
                    let _ = ring.try_push(black_box(i));
                }
                while ring.try_pop().is_ok() {}
            });
        },
    ));

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

#[expect(clippy::too_many_lines)]
fn bench_copy_ring(c: &mut MantisC) {
    let mut descs: Vec<BenchDesc> = Vec::new();

    // --- Copy ring: single push+pop ---
    descs.push(run_bench(c, "copy/single/u64", "u64", 1024, |b| {
        let mut ring = SpscRingCopy::<u64, 1024>::new();
        b.iter(|| {
            let _ = ring.push(black_box(&42u64));
            let mut out = 0u64;
            let _ = ring.pop(black_box(&mut out));
        });
    }));

    descs.push(run_bench(c, "copy/single/msg48", "Message48", 1024, |b| {
        let mut ring = SpscRingCopy::<Message48, 1024>::new();
        let msg = make_msg48(1);
        b.iter(|| {
            let _ = ring.push(black_box(&msg));
            let mut out = Message48::default();
            let _ = ring.pop(black_box(&mut out));
        });
    }));

    descs.push(run_bench(c, "copy/single/msg64", "Message64", 1024, |b| {
        let mut ring = SpscRingCopy::<Message64, 1024>::new();
        let msg = make_msg64(1);
        b.iter(|| {
            let _ = ring.push(black_box(&msg));
            let mut out = Message64::default();
            let _ = ring.pop(black_box(&mut out));
        });
    }));

    // --- Copy ring: burst ---
    descs.push(run_bench(
        c,
        "copy/burst/100/msg48",
        "Message48",
        1024,
        |b| {
            let mut ring = SpscRingCopy::<Message48, 1024>::new();
            let msgs: Vec<Message48> = (0..100).map(make_msg48).collect();
            b.iter(|| burst_copy(&mut ring, black_box(&msgs), 100));
        },
    ));

    descs.push(run_bench(
        c,
        "copy/burst/1000/msg48",
        "Message48",
        2048,
        |b| {
            let mut ring = SpscRingCopy::<Message48, 2048>::new();
            let msgs: Vec<Message48> = (0..1000).map(make_msg48).collect();
            b.iter(|| burst_copy(&mut ring, black_box(&msgs), 1000));
        },
    ));

    // --- Copy ring: batch push+pop ---
    descs.push(run_bench(
        c,
        "copy/batch/100/msg48",
        "Message48",
        1024,
        |b| {
            let mut ring = SpscRingCopy::<Message48, 1024>::new();
            let msgs: Vec<Message48> = (0..100).map(make_msg48).collect();
            b.iter(|| batch_copy(&mut ring, black_box(&msgs), 100));
        },
    ));

    descs.push(run_bench(
        c,
        "copy/batch/1000/msg48",
        "Message48",
        2048,
        |b| {
            let mut ring = SpscRingCopy::<Message48, 2048>::new();
            let msgs: Vec<Message48> = (0..1000).map(make_msg48).collect();
            b.iter(|| batch_copy(&mut ring, black_box(&msgs), 1000));
        },
    ));

    // --- General ring: Message comparison baselines ---
    descs.push(run_bench(
        c,
        "general/single/msg48",
        "Message48",
        1024,
        |b| {
            let mut ring = SpscRing::<Message48, 1024>::new();
            let msg = make_msg48(1);
            b.iter(|| {
                let _ = ring.try_push(black_box(msg));
                let _ = black_box(ring.try_pop());
            });
        },
    ));

    descs.push(run_bench(
        c,
        "general/single/msg64",
        "Message64",
        1024,
        |b| {
            let mut ring = SpscRing::<Message64, 1024>::new();
            let msg = make_msg64(1);
            b.iter(|| {
                let _ = ring.try_push(black_box(msg));
                let _ = black_box(ring.try_pop());
            });
        },
    ));

    let mut features = Vec::new();
    if cfg!(feature = "asm") {
        features.push("asm".to_owned());
    }
    export_report(&descs, "SpscRingCopy (SIMD)", "copy-ring", features);
}

criterion_group! {
    name = benches;
    config = mantis_criterion();
    targets = bench_all, bench_copy_ring
}

criterion::criterion_main!(benches);
