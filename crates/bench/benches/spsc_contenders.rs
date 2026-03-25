//! Criterion benchmarks for external SPSC ring contenders.
//!
//! Requires `bench-contenders` feature flag.
//! Uses `MantisMeasurement` with platform cycle counters.
//! Exports a combined comparison report to
//! `target/bench-report-contenders.json`.

#![allow(missing_docs, clippy::print_stderr)]

use criterion::{black_box, criterion_group};
use mantis_bench::bench_runner::{
    export_report, mantis_criterion, run_bench, BenchDesc, MantisC,
};
use mantis_bench::messages::{make_msg48, make_msg64};

fn bench_contenders(c: &mut MantisC) {
    let mut descs: Vec<BenchDesc> = Vec::new();

    // -- rtrb --
    descs.push(run_bench(c, "spsc/rtrb/single_item/u64", "u64", 1024, |b| {
        let (mut tx, mut rx) = rtrb::RingBuffer::new(1024);
        b.iter(|| {
            let _ = tx.push(black_box(42u64));
            let _ = black_box(rx.pop());
        });
    }));

    descs.push(run_bench(c, "spsc/rtrb/burst_100/u64", "u64", 1024, |b| {
        let (mut tx, mut rx) = rtrb::RingBuffer::new(1024);
        b.iter(|| {
            for i in 0..100u64 {
                let _ = tx.push(black_box(i));
            }
            for _ in 0..100 {
                let _ = black_box(rx.pop());
            }
        });
    }));

    descs.push(run_bench(c, "spsc/rtrb/single_item/msg48", "Message48", 1024, |b| {
        let (mut tx, mut rx) = rtrb::RingBuffer::new(1024);
        let msg = make_msg48(1);
        b.iter(|| {
            let _ = tx.push(black_box(msg));
            let _ = black_box(rx.pop());
        });
    }));

    descs.push(run_bench(c, "spsc/rtrb/single_item/msg64", "Message64", 1024, |b| {
        let (mut tx, mut rx) = rtrb::RingBuffer::new(1024);
        let msg = make_msg64(1);
        b.iter(|| {
            let _ = tx.push(black_box(msg));
            let _ = black_box(rx.pop());
        });
    }));

    // -- crossbeam --
    descs.push(run_bench(
        c,
        "spsc/crossbeam/single_item/u64",
        "u64",
        1024,
        |b| {
            let q = crossbeam_queue::ArrayQueue::new(1024);
            b.iter(|| {
                let _ = q.push(black_box(42u64));
                let _ = black_box(q.pop());
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/crossbeam/burst_100/u64",
        "u64",
        1024,
        |b| {
            let q = crossbeam_queue::ArrayQueue::new(1024);
            b.iter(|| {
                for i in 0..100u64 {
                    let _ = q.push(black_box(i));
                }
                for _ in 0..100 {
                    let _ = black_box(q.pop());
                }
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/crossbeam/single_item/msg48",
        "Message48",
        1024,
        |b| {
            let q = crossbeam_queue::ArrayQueue::new(1024);
            let msg = make_msg48(1);
            b.iter(|| {
                let _ = q.push(black_box(msg));
                let _ = black_box(q.pop());
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/crossbeam/single_item/msg64",
        "Message64",
        1024,
        |b| {
            let q = crossbeam_queue::ArrayQueue::new(1024);
            let msg = make_msg64(1);
            b.iter(|| {
                let _ = q.push(black_box(msg));
                let _ = black_box(q.pop());
            });
        },
    ));

    export_report(
        &descs,
        "Contenders (rtrb, crossbeam)",
        "contenders",
        Vec::new(),
    );
}

criterion_group! {
    name = contenders;
    config = mantis_criterion();
    targets = bench_contenders
}

criterion::criterion_main!(contenders);
