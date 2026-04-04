//! Multi-threaded torn-read detection test.
//!
//! Uses a checksum payload: if any read is torn (partial write),
//! the checksum will be invalid.
//!
//! # Unsafe Usage
//!
//! This test deliberately uses raw pointer aliasing to share the seqlock between
//! a writer (&mut) and multiple readers (&) across threads. This is the canonical
//! pattern for testing seqlocks: the invariant we rely on is that the seqlock
//! protocol itself serializes concurrent access, so the aliasing is sound.
#![allow(unsafe_code)]

use mantis_seqlock::SeqLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

/// Payload where a + b + c + d must always equal CHECKSUM.
/// A torn read would break this invariant.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
struct Payload {
    a: u64,
    b: u64,
    c: u64,
    d: u64,
}

impl Payload {
    const CHECKSUM: u64 = 0xDEAD_BEEF;

    fn new(val: u64) -> Self {
        Self {
            a: val,
            b: val.wrapping_mul(2),
            c: val.wrapping_mul(3),
            d: Self::CHECKSUM
                .wrapping_sub(val)
                .wrapping_sub(val.wrapping_mul(2))
                .wrapping_sub(val.wrapping_mul(3)),
        }
    }

    fn is_valid(&self) -> bool {
        self.a
            .wrapping_add(self.b)
            .wrapping_add(self.c)
            .wrapping_add(self.d)
            == Self::CHECKSUM
    }
}

#[test]
#[expect(clippy::expect_used, reason = "test code: panicking on thread join failure is correct")]
fn no_torn_reads_under_contention() {
    const WRITE_ITERS: u64 = 500_000;
    const NUM_READERS: usize = 4;

    // We need shared access for readers (&self) and exclusive for writer (&mut self).
    // Use Box::into_raw to get a raw pointer, then reconstruct references.
    // This is safe because we enforce: one &mut writer, N & readers, no overlap.
    let lock = Box::into_raw(Box::new(SeqLock::<Payload>::new(Payload::new(0))));
    let running = Box::into_raw(Box::new(AtomicBool::new(true)));

    let lock_addr = lock as usize;
    let running_addr = running as usize;

    let readers: Vec<_> = (0..NUM_READERS)
        .map(|_| {
            let lp = lock_addr;
            let rp = running_addr;
            thread::spawn(move || {
                let lock = unsafe { &*(lp as *const SeqLock<Payload>) };
                let running = unsafe { &*(rp as *const AtomicBool) };
                let mut reads = 0u64;
                while running.load(Ordering::Relaxed) {
                    let p = lock.load();
                    assert!(
                        p.is_valid(),
                        "TORN READ DETECTED at read {reads}: a={} b={} c={} d={} sum={}",
                        p.a,
                        p.b,
                        p.c,
                        p.d,
                        p.a.wrapping_add(p.b).wrapping_add(p.c).wrapping_add(p.d)
                    );
                    reads += 1;
                }
                reads
            })
        })
        .collect();

    // Writer — single thread with &mut
    let writer_lock = unsafe { &mut *lock };
    for i in 0..WRITE_ITERS {
        writer_lock.store(Payload::new(i));
    }

    // Signal readers to stop
    unsafe { &*running }.store(false, Ordering::Relaxed);

    let mut total_reads = 0u64;
    for r in readers {
        total_reads += r.join().expect("reader thread panicked");
    }

    // Cleanup
    unsafe {
        drop(Box::from_raw(lock));
        drop(Box::from_raw(running));
    }

    assert!(total_reads > 0, "readers should have completed some reads");
}
