//! Unified Criterion benchmarks for all SPSC ring implementations.
//!
//! Includes Mantis (inline + copy ring) and optionally external contenders
//! (rtrb, crossbeam) behind the `bench-contenders` feature flag.
//!
//! Produces a single criterion output for CI benchmark regression tracking.
//!
//! ## Workload matrix
//!
//! Every implementation tests the same shapes for fair comparison:
//!
//! | Workload      | u64 | msg48 | msg64 | inline | copy | rtrb | crossbeam | rigtorp | drogalis |
//! |---------------|-----|-------|-------|--------|------|------|-----------|---------|----------|
//! | single        |  x  |   x   |   x   |   x    |  x   |  x   |     x     |    x    |    x     |
//! | burst_100     |  x  |   x   |   x   |   x    |  x   |  x   |     x     |    x    |    x     |
//! | burst_1000    |  x  |   x   |   x   |   x    |  x   |  x   |     x     |    x    |    x     |
//! | batch_100     |  x  |   x   |       |        |  x   |      |           |         |
//! | batch_1000    |  x  |   x   |       |        |  x   |      |           |         |
//! | full_drain    |  x  |       |       |   x    |      |      |           |         |

use std::hint::black_box;

use criterion::criterion_group;
use mantis_bench::bench_runner::{BenchDesc, MantisC, export_report, mantis_criterion, run_bench};
use mantis_bench::messages::{Message48, Message64, make_msg48, make_msg64};
use mantis_bench::workloads::{batch_copy, burst_copy};
use mantis_queue::{SpscRing, SpscRingCopy};

// ---------------------------------------------------------------------------
// Mantis inline ring (general T, move semantics)
// ---------------------------------------------------------------------------

fn bench_inline(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
    bench_inline_single(descs, c);
    bench_inline_burst(descs, c);
}

fn bench_inline_single(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
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
        "spsc/inline/single_item/msg48",
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
        "spsc/inline/single_item/msg64",
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

fn bench_inline_burst(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
    bench_inline_burst_100(descs, c);
    bench_inline_burst_1000(descs, c);

    // -- full drain (mantis-only) --
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
}

fn bench_inline_burst_100(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
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
        "spsc/inline/burst_100/msg48",
        "Message48",
        1024,
        |b| {
            let mut ring = SpscRing::<Message48, 1024>::new();
            let msgs: Vec<Message48> = (0..100).map(make_msg48).collect();
            b.iter(|| {
                for msg in &msgs {
                    let _ = ring.try_push(black_box(*msg));
                }
                for _ in 0..100 {
                    let _ = black_box(ring.try_pop());
                }
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/inline/burst_100/msg64",
        "Message64",
        1024,
        |b| {
            let mut ring = SpscRing::<Message64, 1024>::new();
            let msgs: Vec<Message64> = (0..100).map(make_msg64).collect();
            b.iter(|| {
                for msg in &msgs {
                    let _ = ring.try_push(black_box(*msg));
                }
                for _ in 0..100 {
                    let _ = black_box(ring.try_pop());
                }
            });
        },
    ));
}

fn bench_inline_burst_1000(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
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
        "spsc/inline/burst_1000/msg48",
        "Message48",
        2048,
        |b| {
            let mut ring = SpscRing::<Message48, 2048>::new();
            let msgs: Vec<Message48> = (0..1000).map(make_msg48).collect();
            b.iter(|| {
                for msg in &msgs {
                    let _ = ring.try_push(black_box(*msg));
                }
                for _ in 0..1000 {
                    let _ = black_box(ring.try_pop());
                }
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/inline/burst_1000/msg64",
        "Message64",
        2048,
        |b| {
            let mut ring = SpscRing::<Message64, 2048>::new();
            let msgs: Vec<Message64> = (0..1000).map(make_msg64).collect();
            b.iter(|| {
                for msg in &msgs {
                    let _ = ring.try_push(black_box(*msg));
                }
                for _ in 0..1000 {
                    let _ = black_box(ring.try_pop());
                }
            });
        },
    ));
}

// ---------------------------------------------------------------------------
// Mantis copy ring (T: Copy, SIMD-optimized)
// ---------------------------------------------------------------------------

fn bench_copy(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
    bench_copy_single(descs, c);
    bench_copy_burst(descs, c);
    bench_copy_batch(descs, c);
}

fn bench_copy_single(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
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

    // general ring baselines (compare move vs copy for same message type)
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

fn bench_copy_burst(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
    // burst 100
    descs.push(run_bench(c, "copy/burst/100/u64", "u64", 1024, |b| {
        let mut ring = SpscRingCopy::<u64, 1024>::new();
        let vals: Vec<u64> = (0..100).collect();
        b.iter(|| burst_copy(&mut ring, black_box(&vals), 100));
    }));

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
        "copy/burst/100/msg64",
        "Message64",
        1024,
        |b| {
            let mut ring = SpscRingCopy::<Message64, 1024>::new();
            let msgs: Vec<Message64> = (0..100).map(make_msg64).collect();
            b.iter(|| burst_copy(&mut ring, black_box(&msgs), 100));
        },
    ));

    // burst 1000
    descs.push(run_bench(c, "copy/burst/1000/u64", "u64", 2048, |b| {
        let mut ring = SpscRingCopy::<u64, 2048>::new();
        let vals: Vec<u64> = (0..1000).collect();
        b.iter(|| burst_copy(&mut ring, black_box(&vals), 1000));
    }));

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

    descs.push(run_bench(
        c,
        "copy/burst/1000/msg64",
        "Message64",
        2048,
        |b| {
            let mut ring = SpscRingCopy::<Message64, 2048>::new();
            let msgs: Vec<Message64> = (0..1000).map(make_msg64).collect();
            b.iter(|| burst_copy(&mut ring, black_box(&msgs), 1000));
        },
    ));
}

fn bench_copy_batch(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
    // batch 100 (mantis-only: contiguous copy_nonoverlapping)
    descs.push(run_bench(c, "copy/batch/100/u64", "u64", 1024, |b| {
        let mut ring = SpscRingCopy::<u64, 1024>::new();
        let vals: Vec<u64> = (0..100).collect();
        b.iter(|| batch_copy(&mut ring, black_box(&vals), 100));
    }));

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

    // batch 1000 (mantis-only)
    descs.push(run_bench(c, "copy/batch/1000/u64", "u64", 2048, |b| {
        let mut ring = SpscRingCopy::<u64, 2048>::new();
        let vals: Vec<u64> = (0..1000).collect();
        b.iter(|| batch_copy(&mut ring, black_box(&vals), 1000));
    }));

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
}

// ---------------------------------------------------------------------------
// Contenders (behind feature flag)
// ---------------------------------------------------------------------------

#[cfg(feature = "bench-contenders")]
fn bench_contenders(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
    bench_rtrb(descs, c);
    bench_crossbeam(descs, c);
}

#[cfg(feature = "bench-contenders")]
fn bench_rtrb(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
    bench_rtrb_single(descs, c);
    bench_rtrb_burst(descs, c);
}

#[cfg(feature = "bench-contenders")]
fn bench_rtrb_single(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
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
}

#[cfg(feature = "bench-contenders")]
fn bench_rtrb_burst(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
    // burst 100
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
        "spsc/rtrb/burst_100/msg48",
        "Message48",
        1024,
        |b| {
            let (mut tx, mut rx) = rtrb::RingBuffer::new(1024);
            let msgs: Vec<Message48> = (0..100).map(make_msg48).collect();
            b.iter(|| {
                for msg in &msgs {
                    let _ = tx.push(black_box(*msg));
                }
                for _ in 0..100 {
                    let _ = black_box(rx.pop());
                }
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/rtrb/burst_100/msg64",
        "Message64",
        1024,
        |b| {
            let (mut tx, mut rx) = rtrb::RingBuffer::new(1024);
            let msgs: Vec<Message64> = (0..100).map(make_msg64).collect();
            b.iter(|| {
                for msg in &msgs {
                    let _ = tx.push(black_box(*msg));
                }
                for _ in 0..100 {
                    let _ = black_box(rx.pop());
                }
            });
        },
    ));

    // burst 1000
    descs.push(run_bench(c, "spsc/rtrb/burst_1000/u64", "u64", 2048, |b| {
        let (mut tx, mut rx) = rtrb::RingBuffer::new(2048);
        b.iter(|| {
            for i in 0..1000u64 {
                let _ = tx.push(black_box(i));
            }
            for _ in 0..1000 {
                let _ = black_box(rx.pop());
            }
        });
    }));

    descs.push(run_bench(
        c,
        "spsc/rtrb/burst_1000/msg48",
        "Message48",
        2048,
        |b| {
            let (mut tx, mut rx) = rtrb::RingBuffer::new(2048);
            let msgs: Vec<Message48> = (0..1000).map(make_msg48).collect();
            b.iter(|| {
                for msg in &msgs {
                    let _ = tx.push(black_box(*msg));
                }
                for _ in 0..1000 {
                    let _ = black_box(rx.pop());
                }
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/rtrb/burst_1000/msg64",
        "Message64",
        2048,
        |b| {
            let (mut tx, mut rx) = rtrb::RingBuffer::new(2048);
            let msgs: Vec<Message64> = (0..1000).map(make_msg64).collect();
            b.iter(|| {
                for msg in &msgs {
                    let _ = tx.push(black_box(*msg));
                }
                for _ in 0..1000 {
                    let _ = black_box(rx.pop());
                }
            });
        },
    ));
}

#[cfg(feature = "bench-contenders")]
fn bench_crossbeam(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
    bench_crossbeam_single(descs, c);
    bench_crossbeam_burst(descs, c);
}

#[cfg(feature = "bench-contenders")]
fn bench_crossbeam_single(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
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

#[cfg(feature = "bench-contenders")]
fn bench_crossbeam_burst(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
    bench_crossbeam_burst_100(descs, c);
    bench_crossbeam_burst_1000(descs, c);
}

#[cfg(feature = "bench-contenders")]
fn bench_crossbeam_burst_100(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
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
        "spsc/crossbeam/burst_100/msg48",
        "Message48",
        1024,
        |b| {
            let q = crossbeam_queue::ArrayQueue::new(1024);
            let msgs: Vec<Message48> = (0..100).map(make_msg48).collect();
            b.iter(|| {
                for msg in &msgs {
                    let _ = q.push(black_box(*msg));
                }
                for _ in 0..100 {
                    let _ = black_box(q.pop());
                }
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/crossbeam/burst_100/msg64",
        "Message64",
        1024,
        |b| {
            let q = crossbeam_queue::ArrayQueue::new(1024);
            let msgs: Vec<Message64> = (0..100).map(make_msg64).collect();
            b.iter(|| {
                for msg in &msgs {
                    let _ = q.push(black_box(*msg));
                }
                for _ in 0..100 {
                    let _ = black_box(q.pop());
                }
            });
        },
    ));
}

#[cfg(feature = "bench-contenders")]
fn bench_crossbeam_burst_1000(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
    descs.push(run_bench(
        c,
        "spsc/crossbeam/burst_1000/u64",
        "u64",
        2048,
        |b| {
            let q = crossbeam_queue::ArrayQueue::new(2048);
            b.iter(|| {
                for i in 0..1000u64 {
                    let _ = q.push(black_box(i));
                }
                for _ in 0..1000 {
                    let _ = black_box(q.pop());
                }
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/crossbeam/burst_1000/msg48",
        "Message48",
        2048,
        |b| {
            let q = crossbeam_queue::ArrayQueue::new(2048);
            let msgs: Vec<Message48> = (0..1000).map(make_msg48).collect();
            b.iter(|| {
                for msg in &msgs {
                    let _ = q.push(black_box(*msg));
                }
                for _ in 0..1000 {
                    let _ = black_box(q.pop());
                }
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/crossbeam/burst_1000/msg64",
        "Message64",
        2048,
        |b| {
            let q = crossbeam_queue::ArrayQueue::new(2048);
            let msgs: Vec<Message64> = (0..1000).map(make_msg64).collect();
            b.iter(|| {
                for msg in &msgs {
                    let _ = q.push(black_box(*msg));
                }
                for _ in 0..1000 {
                    let _ = black_box(q.pop());
                }
            });
        },
    ));
}

#[cfg(not(feature = "bench-contenders"))]
fn bench_contenders(_descs: &mut Vec<BenchDesc>, _c: &mut MantisC) {}

// ---------------------------------------------------------------------------
// C++ contenders (behind feature flag)
// ---------------------------------------------------------------------------

#[cfg(feature = "bench-contenders-cpp")]
fn bench_cpp_contenders(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
    bench_rigtorp_single(descs, c);
    bench_rigtorp_burst(descs, c);
    bench_drogalis(descs, c);
}

#[cfg(feature = "bench-contenders-cpp")]
fn bench_rigtorp_single(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
    use mantis_bench::rigtorp_ffi::{RigtorpMsg48, RigtorpMsg64, RigtorpU64};

    descs.push(run_bench(
        c,
        "spsc/rigtorp/single_item/u64",
        "u64",
        1024,
        |b| {
            let mut q = RigtorpU64::new(1024);
            b.iter(|| {
                let _ = q.try_push(black_box(42u64));
                let _ = black_box(q.try_pop());
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/rigtorp/single_item/msg48",
        "Message48",
        1024,
        |b| {
            let mut q = RigtorpMsg48::new(1024);
            let msg = make_msg48(1);
            b.iter(|| {
                let _ = q.try_push(black_box(&msg));
                let _ = black_box(q.try_pop());
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/rigtorp/single_item/msg64",
        "Message64",
        1024,
        |b| {
            let mut q = RigtorpMsg64::new(1024);
            let msg = make_msg64(1);
            b.iter(|| {
                let _ = q.try_push(black_box(&msg));
                let _ = black_box(q.try_pop());
            });
        },
    ));
}

#[cfg(feature = "bench-contenders-cpp")]
fn bench_rigtorp_burst(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
    bench_rigtorp_burst_100(descs, c);
    bench_rigtorp_burst_1000(descs, c);
}

#[cfg(feature = "bench-contenders-cpp")]
fn bench_rigtorp_burst_100(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
    use mantis_bench::rigtorp_ffi::{RigtorpMsg48, RigtorpMsg64, RigtorpU64};

    descs.push(run_bench(
        c,
        "spsc/rigtorp/burst_100/u64",
        "u64",
        1024,
        |b| {
            let mut q = RigtorpU64::new(1024);
            b.iter(|| {
                for i in 0..100u64 {
                    let _ = q.try_push(black_box(i));
                }
                for _ in 0..100 {
                    let _ = black_box(q.try_pop());
                }
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/rigtorp/burst_100/msg48",
        "Message48",
        1024,
        |b| {
            let mut q = RigtorpMsg48::new(1024);
            let msgs: Vec<Message48> = (0..100).map(make_msg48).collect();
            b.iter(|| {
                for msg in &msgs {
                    let _ = q.try_push(black_box(msg));
                }
                for _ in 0..100 {
                    let _ = black_box(q.try_pop());
                }
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/rigtorp/burst_100/msg64",
        "Message64",
        1024,
        |b| {
            let mut q = RigtorpMsg64::new(1024);
            let msgs: Vec<Message64> = (0..100).map(make_msg64).collect();
            b.iter(|| {
                for msg in &msgs {
                    let _ = q.try_push(black_box(msg));
                }
                for _ in 0..100 {
                    let _ = black_box(q.try_pop());
                }
            });
        },
    ));
}

#[cfg(feature = "bench-contenders-cpp")]
fn bench_rigtorp_burst_1000(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
    use mantis_bench::rigtorp_ffi::{RigtorpMsg48, RigtorpMsg64, RigtorpU64};

    descs.push(run_bench(
        c,
        "spsc/rigtorp/burst_1000/u64",
        "u64",
        2048,
        |b| {
            let mut q = RigtorpU64::new(2048);
            b.iter(|| {
                for i in 0..1000u64 {
                    let _ = q.try_push(black_box(i));
                }
                for _ in 0..1000 {
                    let _ = black_box(q.try_pop());
                }
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/rigtorp/burst_1000/msg48",
        "Message48",
        2048,
        |b| {
            let mut q = RigtorpMsg48::new(2048);
            let msgs: Vec<Message48> = (0..1000).map(make_msg48).collect();
            b.iter(|| {
                for msg in &msgs {
                    let _ = q.try_push(black_box(msg));
                }
                for _ in 0..1000 {
                    let _ = black_box(q.try_pop());
                }
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/rigtorp/burst_1000/msg64",
        "Message64",
        2048,
        |b| {
            let mut q = RigtorpMsg64::new(2048);
            let msgs: Vec<Message64> = (0..1000).map(make_msg64).collect();
            b.iter(|| {
                for msg in &msgs {
                    let _ = q.try_push(black_box(msg));
                }
                for _ in 0..1000 {
                    let _ = black_box(q.try_pop());
                }
            });
        },
    ));
}

#[cfg(feature = "bench-contenders-cpp")]
fn bench_drogalis(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
    bench_drogalis_single(descs, c);
    bench_drogalis_burst(descs, c);
}

#[cfg(feature = "bench-contenders-cpp")]
fn bench_drogalis_single(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
    use mantis_bench::drogalis_ffi::{DrogalisMsg48, DrogalisMsg64, DrogalisU64};

    descs.push(run_bench(
        c,
        "spsc/drogalis/single_item/u64",
        "u64",
        1024,
        |b| {
            let mut q = DrogalisU64::new(1024);
            b.iter(|| {
                let _ = q.try_push(black_box(42u64));
                let _ = black_box(q.try_pop());
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/drogalis/single_item/msg48",
        "Message48",
        1024,
        |b| {
            let mut q = DrogalisMsg48::new(1024);
            let msg = make_msg48(1);
            b.iter(|| {
                let _ = q.try_push(black_box(&msg));
                let _ = black_box(q.try_pop());
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/drogalis/single_item/msg64",
        "Message64",
        1024,
        |b| {
            let mut q = DrogalisMsg64::new(1024);
            let msg = make_msg64(1);
            b.iter(|| {
                let _ = q.try_push(black_box(&msg));
                let _ = black_box(q.try_pop());
            });
        },
    ));
}

#[cfg(feature = "bench-contenders-cpp")]
fn bench_drogalis_burst(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
    bench_drogalis_burst_100(descs, c);
    bench_drogalis_burst_1000(descs, c);
}

#[cfg(feature = "bench-contenders-cpp")]
fn bench_drogalis_burst_100(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
    use mantis_bench::drogalis_ffi::{DrogalisMsg48, DrogalisMsg64, DrogalisU64};

    descs.push(run_bench(
        c,
        "spsc/drogalis/burst_100/u64",
        "u64",
        1024,
        |b| {
            let mut q = DrogalisU64::new(1024);
            b.iter(|| {
                for i in 0..100u64 {
                    let _ = q.try_push(black_box(i));
                }
                for _ in 0..100 {
                    let _ = black_box(q.try_pop());
                }
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/drogalis/burst_100/msg48",
        "Message48",
        1024,
        |b| {
            let mut q = DrogalisMsg48::new(1024);
            let msgs: Vec<Message48> = (0..100).map(make_msg48).collect();
            b.iter(|| {
                for msg in &msgs {
                    let _ = q.try_push(black_box(msg));
                }
                for _ in 0..100 {
                    let _ = black_box(q.try_pop());
                }
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/drogalis/burst_100/msg64",
        "Message64",
        1024,
        |b| {
            let mut q = DrogalisMsg64::new(1024);
            let msgs: Vec<Message64> = (0..100).map(make_msg64).collect();
            b.iter(|| {
                for msg in &msgs {
                    let _ = q.try_push(black_box(msg));
                }
                for _ in 0..100 {
                    let _ = black_box(q.try_pop());
                }
            });
        },
    ));
}

#[cfg(feature = "bench-contenders-cpp")]
fn bench_drogalis_burst_1000(descs: &mut Vec<BenchDesc>, c: &mut MantisC) {
    use mantis_bench::drogalis_ffi::{DrogalisMsg48, DrogalisMsg64, DrogalisU64};

    descs.push(run_bench(
        c,
        "spsc/drogalis/burst_1000/u64",
        "u64",
        2048,
        |b| {
            let mut q = DrogalisU64::new(2048);
            b.iter(|| {
                for i in 0..1000u64 {
                    let _ = q.try_push(black_box(i));
                }
                for _ in 0..1000 {
                    let _ = black_box(q.try_pop());
                }
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/drogalis/burst_1000/msg48",
        "Message48",
        2048,
        |b| {
            let mut q = DrogalisMsg48::new(2048);
            let msgs: Vec<Message48> = (0..1000).map(make_msg48).collect();
            b.iter(|| {
                for msg in &msgs {
                    let _ = q.try_push(black_box(msg));
                }
                for _ in 0..1000 {
                    let _ = black_box(q.try_pop());
                }
            });
        },
    ));

    descs.push(run_bench(
        c,
        "spsc/drogalis/burst_1000/msg64",
        "Message64",
        2048,
        |b| {
            let mut q = DrogalisMsg64::new(2048);
            let msgs: Vec<Message64> = (0..1000).map(make_msg64).collect();
            b.iter(|| {
                for msg in &msgs {
                    let _ = q.try_push(black_box(msg));
                }
                for _ in 0..1000 {
                    let _ = black_box(q.try_pop());
                }
            });
        },
    ));
}

#[cfg(not(feature = "bench-contenders-cpp"))]
fn bench_cpp_contenders(_descs: &mut Vec<BenchDesc>, _c: &mut MantisC) {}

// ---------------------------------------------------------------------------
// Unified entry point
// ---------------------------------------------------------------------------

fn bench_spsc(c: &mut MantisC) {
    let mut descs: Vec<BenchDesc> = Vec::new();

    bench_inline(&mut descs, c);
    bench_copy(&mut descs, c);
    bench_contenders(&mut descs, c);
    bench_cpp_contenders(&mut descs, c);

    let mut features = Vec::new();
    if cfg!(feature = "asm") {
        features.push("asm".to_owned());
    }
    if cfg!(feature = "perf-counters") {
        features.push("perf-counters".to_owned());
    }
    if cfg!(feature = "bench-contenders") {
        features.push("bench-contenders".to_owned());
    }
    if cfg!(feature = "bench-contenders-cpp") {
        features.push("bench-contenders-cpp".to_owned());
    }
    export_report(&descs, "SPSC Ring (all)", "spsc", features);
}

criterion_group! {
    name = spsc;
    config = mantis_criterion();
    targets = bench_spsc
}

criterion::criterion_main!(spsc);
