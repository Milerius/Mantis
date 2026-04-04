//! Criterion benchmarks for mantis-seqlock.

#![allow(unsafe_code)]
#![allow(clippy::unwrap_used)]

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use mantis_seqlock::SeqLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct Msg64([u8; 64]);

#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct Msg128([u8; 128]);

fn bench_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("seqlock/write");

    group.bench_function("u64", |b| {
        let mut lock = SeqLock::<u64>::new(0);
        let mut i = 0u64;
        b.iter(|| {
            i = i.wrapping_add(1);
            lock.store(black_box(i));
        });
    });

    group.bench_function("msg64", |b| {
        let mut lock = SeqLock::<Msg64>::new(Msg64([0; 64]));
        b.iter(|| {
            lock.store(black_box(Msg64([0xAB; 64])));
        });
    });

    group.bench_function("msg128", |b| {
        let mut lock = SeqLock::<Msg128>::new(Msg128([0; 128]));
        b.iter(|| {
            lock.store(black_box(Msg128([0xCD; 128])));
        });
    });

    group.finish();
}

fn bench_read_uncontended(c: &mut Criterion) {
    let mut group = c.benchmark_group("seqlock/read_uncontended");

    group.bench_function("u64", |b| {
        let lock = SeqLock::<u64>::new(42);
        b.iter(|| {
            black_box(lock.load());
        });
    });

    group.bench_function("msg64", |b| {
        let lock = SeqLock::<Msg64>::new(Msg64([0xAB; 64]));
        b.iter(|| {
            black_box(lock.load());
        });
    });

    group.bench_function("msg128", |b| {
        let lock = SeqLock::<Msg128>::new(Msg128([0xCD; 128]));
        b.iter(|| {
            black_box(lock.load());
        });
    });

    group.finish();
}

fn bench_read_contended(c: &mut Criterion) {
    let mut group = c.benchmark_group("seqlock/read_contended");

    group.bench_function("u64", |b| {
        let lock = Box::into_raw(Box::new(SeqLock::<u64>::new(0)));
        let running = Box::into_raw(Box::new(AtomicBool::new(true)));

        let lp = lock as usize;
        let rp = running as usize;
        let writer = thread::spawn(move || {
            // SAFETY: pointer is valid for the duration of the benchmark iteration;
            // the main thread joins this thread before dropping the allocation.
            let lock = unsafe { &mut *(lp as *mut SeqLock<u64>) };
            // SAFETY: same lifetime guarantee as above.
            let running = unsafe { &*(rp as *const AtomicBool) };
            let mut i = 0u64;
            while running.load(Ordering::Relaxed) {
                i = i.wrapping_add(1);
                lock.store(i);
            }
        });

        // SAFETY: allocation is still live; writer thread holds a separate reference
        // to the same allocation but SeqLock<u64>: Sync so shared &self is valid here.
        let lock_ref = unsafe { &*lock };
        b.iter(|| {
            black_box(lock_ref.load());
        });

        // SAFETY: same allocation; AtomicBool is Sync.
        unsafe { &*running }.store(false, Ordering::Relaxed);
        writer.join().unwrap();
        // SAFETY: writer thread has exited; we are the sole owner again.
        unsafe {
            drop(Box::from_raw(lock));
            drop(Box::from_raw(running));
        }
    });

    group.bench_function("msg64", |b| {
        let lock = Box::into_raw(Box::new(SeqLock::<Msg64>::new(Msg64([0; 64]))));
        let running = Box::into_raw(Box::new(AtomicBool::new(true)));

        let lp = lock as usize;
        let rp = running as usize;
        let writer = thread::spawn(move || {
            // SAFETY: pointer is valid until writer.join(); main thread joins before drop.
            let lock = unsafe { &mut *(lp as *mut SeqLock<Msg64>) };
            // SAFETY: same lifetime guarantee.
            let running = unsafe { &*(rp as *const AtomicBool) };
            while running.load(Ordering::Relaxed) {
                lock.store(Msg64([0xAB; 64]));
            }
        });

        // SAFETY: allocation live; SeqLock<Msg64>: Sync.
        let lock_ref = unsafe { &*lock };
        b.iter(|| {
            black_box(lock_ref.load());
        });

        // SAFETY: allocation live; AtomicBool: Sync.
        unsafe { &*running }.store(false, Ordering::Relaxed);
        writer.join().unwrap();
        // SAFETY: writer thread has exited; sole owner.
        unsafe {
            drop(Box::from_raw(lock));
            drop(Box::from_raw(running));
        }
    });

    group.finish();
}

#[cfg(feature = "bench-seqlock-contenders")]
fn bench_contender_amanieu(c: &mut Criterion) {
    use ::seqlock::SeqLock as AmanieuSeqLock;

    let mut group = c.benchmark_group("seqlock/contender/amanieu");

    // u64
    group.bench_function("write_u64", |b| {
        let lock = AmanieuSeqLock::new(0u64);
        let mut i = 0u64;
        b.iter(|| {
            i = i.wrapping_add(1);
            *lock.lock_write() = black_box(i);
        });
    });
    group.bench_function("read_u64", |b| {
        let lock = AmanieuSeqLock::new(42u64);
        b.iter(|| black_box(lock.read()));
    });

    // msg64
    group.bench_function("write_msg64", |b| {
        let lock = AmanieuSeqLock::new(Msg64([0; 64]));
        b.iter(|| {
            *lock.lock_write() = black_box(Msg64([0xAB; 64]));
        });
    });
    group.bench_function("read_msg64", |b| {
        let lock = AmanieuSeqLock::new(Msg64([0xAB; 64]));
        b.iter(|| black_box(lock.read()));
    });

    // msg128
    group.bench_function("write_msg128", |b| {
        let lock = AmanieuSeqLock::new(Msg128([0; 128]));
        b.iter(|| {
            *lock.lock_write() = black_box(Msg128([0xCD; 128]));
        });
    });
    group.bench_function("read_msg128", |b| {
        let lock = AmanieuSeqLock::new(Msg128([0xCD; 128]));
        b.iter(|| black_box(lock.read()));
    });

    group.finish();
}

#[cfg(feature = "bench-seqlock-contenders-cpp")]
fn bench_contender_rigtorp(c: &mut Criterion) {
    use mantis_bench::seqlock_ffi::{
        BenchMsg64, BenchMsg128, rigtorp_seqlock_read_64, rigtorp_seqlock_read_128,
        rigtorp_seqlock_read_u64, rigtorp_seqlock_write_64, rigtorp_seqlock_write_128,
        rigtorp_seqlock_write_u64,
    };

    let mut group = c.benchmark_group("seqlock/contender/rigtorp");

    // u64
    group.bench_function("write_u64", |b| {
        let mut i = 0u64;
        b.iter(|| {
            i = i.wrapping_add(1);
            unsafe { rigtorp_seqlock_write_u64(black_box(i)) };
        });
    });
    group.bench_function("read_u64", |b| {
        b.iter(|| black_box(unsafe { rigtorp_seqlock_read_u64() }));
    });

    // msg64
    group.bench_function("write_msg64", |b| {
        let val = BenchMsg64 { data: [0xAB; 64] };
        b.iter(|| unsafe { rigtorp_seqlock_write_64(black_box(core::ptr::addr_of!(val))) });
    });
    group.bench_function("read_msg64", |b| {
        let mut out = BenchMsg64 { data: [0; 64] };
        b.iter(|| unsafe { rigtorp_seqlock_read_64(black_box(core::ptr::addr_of_mut!(out))) });
    });

    // msg128
    group.bench_function("write_msg128", |b| {
        let val = BenchMsg128 { data: [0xCD; 128] };
        b.iter(|| unsafe { rigtorp_seqlock_write_128(black_box(core::ptr::addr_of!(val))) });
    });
    group.bench_function("read_msg128", |b| {
        let mut out = BenchMsg128 { data: [0; 128] };
        b.iter(|| unsafe { rigtorp_seqlock_read_128(black_box(core::ptr::addr_of_mut!(out))) });
    });

    group.finish();
}

#[cfg(not(feature = "bench-seqlock-contenders"))]
fn bench_contender_amanieu(_c: &mut Criterion) {}

#[cfg(not(feature = "bench-seqlock-contenders-cpp"))]
fn bench_contender_rigtorp(_c: &mut Criterion) {}

criterion_group!(
    benches,
    bench_write,
    bench_read_uncontended,
    bench_read_contended,
    bench_contender_amanieu,
    bench_contender_rigtorp
);
criterion_main!(benches);
