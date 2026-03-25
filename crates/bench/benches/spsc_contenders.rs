//! Criterion benchmarks for external SPSC ring contenders.
//!
//! Requires `bench-contenders` feature flag.

#![allow(missing_docs)]

use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_rtrb_single_item(c: &mut Criterion) {
    c.bench_function("spsc/rtrb/single_item/u64", |b| {
        let (mut tx, mut rx) = rtrb::RingBuffer::new(1024);
        b.iter(|| {
            let _ = tx.push(black_box(42u64));
            let _ = black_box(rx.pop());
        });
    });
}

fn bench_crossbeam_single_item(c: &mut Criterion) {
    c.bench_function("spsc/crossbeam/single_item/u64", |b| {
        let q = crossbeam_queue::ArrayQueue::new(1024);
        b.iter(|| {
            let _ = q.push(black_box(42u64));
            let _ = black_box(q.pop());
        });
    });
}

fn bench_rtrb_burst(c: &mut Criterion) {
    c.bench_function("spsc/rtrb/burst_100/u64", |b| {
        let (mut tx, mut rx) = rtrb::RingBuffer::new(1024);
        b.iter(|| {
            for i in 0..100u64 {
                let _ = tx.push(black_box(i));
            }
            for _ in 0..100 {
                let _ = black_box(rx.pop());
            }
        });
    });
}

fn bench_crossbeam_burst(c: &mut Criterion) {
    c.bench_function("spsc/crossbeam/burst_100/u64", |b| {
        let q = crossbeam_queue::ArrayQueue::new(1024);
        b.iter(|| {
            for i in 0..100u64 {
                let _ = q.push(black_box(i));
            }
            for _ in 0..100 {
                let _ = black_box(q.pop());
            }
        });
    });
}

criterion_group!(
    contenders,
    bench_rtrb_single_item,
    bench_crossbeam_single_item,
    bench_rtrb_burst,
    bench_crossbeam_burst,
);
criterion_main!(contenders);
