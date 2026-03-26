//! Unified Criterion benchmarks for all SPSC ring implementations.
//!
//! Includes Mantis (inline + copy ring) and optionally external contenders
//! (rtrb, crossbeam) behind the `bench-contenders` feature flag.
//!
//! Produces a single criterion output for CI benchmark regression tracking.

#![expect(missing_docs, clippy::print_stderr, reason = "benchmark binary")]

use criterion::{black_box, criterion_group};
use mantis_bench::bench_runner::{BenchDesc, MantisC, export_report, mantis_criterion, run_bench};
use mantis_bench::messages::{Message48, Message64, make_msg48, make_msg64};
use mantis_bench::workloads::{batch_copy, burst_copy};
use mantis_queue::{SpscRing, SpscRingCopy};

// ---------------------------------------------------------------------------
// Mantis inline ring
// ---------------------------------------------------------------------------

fn bench_inline(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
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
}

// ---------------------------------------------------------------------------
// Mantis copy ring
// ---------------------------------------------------------------------------

fn bench_copy(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
    // Single push+pop
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

    // Burst
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

    // Batch
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

    // General ring baselines for message types
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
}

// ---------------------------------------------------------------------------
// Contenders (behind feature flag)
// ---------------------------------------------------------------------------

#[cfg(feature = "bench-contenders")]
#[expect(clippy::too_many_lines)]
fn bench_contenders(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
    // -- rtrb --
    descs.push(run_bench(
        c,
        "spsc/rtrb/single_item/u64",
        "u64",
        1024,
        |b| {
            let (mut tx, mut rx) = rtrb::RingBuffer::new(1024);
            b.iter(|| {
                let _ = tx.push(black_box(42u64));
                let _ = black_box(rx.pop());
            });
        },
    ));

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

    descs.push(run_bench(
        c,
        "spsc/rtrb/single_item/msg48",
        "Message48",
        1024,
        |b| {
            let (mut tx, mut rx) = rtrb::RingBuffer::new(1024);
            let msg = make_msg48(1);
            b.iter(|| {
                let _ = tx.push(black_box(msg));
                let _ = black_box(rx.pop());
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/rtrb/single_item/msg64",
        "Message64",
        1024,
        |b| {
            let (mut tx, mut rx) = rtrb::RingBuffer::new(1024);
            let msg = make_msg64(1);
            b.iter(|| {
                let _ = tx.push(black_box(msg));
                let _ = black_box(rx.pop());
            });
        },
    ));

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
}

#[cfg(not(feature = "bench-contenders"))]
fn bench_contenders(_descs: &mut Vec<BenchDesc>, _c: &mut MantisC) {}

// ---------------------------------------------------------------------------
// Unified entry point
// ---------------------------------------------------------------------------

fn bench_spsc(c: &mut MantisC) {
    let mut descs: Vec<BenchDesc> = Vec::new();

    bench_inline(&mut descs, c);
    bench_copy(&mut descs, c);
    bench_contenders(&mut descs, c);

    let mut features = Vec::new();
    if cfg!(feature = "asm") {
        features.push("asm".to_owned());
    }
    if cfg!(feature = "bench-contenders") {
        features.push("bench-contenders".to_owned());
    }
    export_report(&descs, "SPSC Ring (all)", "spsc", features);
}

criterion_group! {
    name = spsc;
    config = mantis_criterion();
    targets = bench_spsc
}

criterion::criterion_main!(spsc);
