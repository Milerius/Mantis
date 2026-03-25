//! Criterion benchmarks for all Mantis SPSC ring presets.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use mantis_queue::SpscRing;

fn bench_single_item(c: &mut Criterion) {
    c.bench_function("spsc/inline/single_item/u64", |b| {
        let mut ring = SpscRing::<u64, 1024>::new();
        b.iter(|| {
            let _ = ring.try_push(black_box(42u64));
            let _ = black_box(ring.try_pop());
        });
    });
}

fn bench_burst_100(c: &mut Criterion) {
    c.bench_function("spsc/inline/burst_100/u64", |b| {
        let mut ring = SpscRing::<u64, 1024>::new();
        b.iter(|| {
            for i in 0..100u64 {
                let _ = ring.try_push(black_box(i));
            }
            for _ in 0..100 {
                let _ = black_box(ring.try_pop());
            }
        });
    });
}

fn bench_full_drain(c: &mut Criterion) {
    c.bench_function("spsc/inline/full_drain/u64", |b| {
        let mut ring = SpscRing::<u64, 1024>::new();
        b.iter(|| {
            for i in 0..1023u64 {
                let _ = ring.try_push(black_box(i));
            }
            while ring.try_pop().is_ok() {}
        });
    });
}

fn bench_burst_1000(c: &mut Criterion) {
    c.bench_function("spsc/inline/burst_1000/u64", |b| {
        let mut ring = SpscRing::<u64, 2048>::new();
        b.iter(|| {
            for i in 0..1000u64 {
                let _ = ring.try_push(black_box(i));
            }
            for _ in 0..1000 {
                let _ = black_box(ring.try_pop());
            }
        });
    });
}

fn bench_single_item_64b(c: &mut Criterion) {
    c.bench_function("spsc/inline/single_item/[u8;64]", |b| {
        let mut ring = SpscRing::<[u8; 64], 1024>::new();
        b.iter(|| {
            let _ = ring.try_push(black_box([0u8; 64]));
            let _ = black_box(ring.try_pop());
        });
    });
}

fn bench_single_item_256b(c: &mut Criterion) {
    c.bench_function("spsc/inline/single_item/[u8;256]", |b| {
        let mut ring = SpscRing::<[u8; 256], 256>::new();
        b.iter(|| {
            let _ = ring.try_push(black_box([0u8; 256]));
            let _ = black_box(ring.try_pop());
        });
    });
}

criterion_group!(
    benches,
    bench_single_item,
    bench_burst_100,
    bench_burst_1000,
    bench_full_drain,
    bench_single_item_64b,
    bench_single_item_256b,
);
criterion_main!(benches);
