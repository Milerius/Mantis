//! Skip under Miri — queue uses inline assembly (`cold_path`, `prefetch`).
//! Integration tests for the copy-optimized SPSC ring.
use mantis_queue::{SpscRingCopy, SpscRingCopyInstrumented};

#[cfg(feature = "alloc")]
use mantis_queue::SpscRingCopyHeap;

#[test]
fn inline_push_pop() {
    let mut ring = SpscRingCopy::<u64, 8>::new();
    assert!(ring.push(&42));
    let mut out = 0u64;
    assert!(ring.pop(&mut out));
    assert_eq!(out, 42);
}

#[test]
fn inline_fill_and_drain() {
    let mut ring = SpscRingCopy::<u64, 8>::new();
    for i in 0u64..7 {
        assert!(ring.push(&i));
    }
    assert!(!ring.push(&99)); // full
    for i in 0u64..7 {
        let mut out = 0u64;
        assert!(ring.pop(&mut out));
        assert_eq!(out, i);
    }
    let mut out = 0u64;
    assert!(!ring.pop(&mut out)); // empty
}

#[test]
fn inline_batch_roundtrip() {
    let mut ring = SpscRingCopy::<u64, 1024>::new();
    let src: Vec<u64> = (0..100).collect();
    assert_eq!(ring.push_batch(&src), 100);
    let mut dst = vec![0u64; 100];
    assert_eq!(ring.pop_batch(&mut dst), 100);
    assert_eq!(src, dst);
}

#[cfg(feature = "alloc")]
#[test]
fn stress_two_thread_message48() {
    use core::hint::spin_loop;
    use mantis_queue::spsc_ring_copy;
    use std::thread;

    #[repr(C, align(16))]
    #[derive(Clone, Copy, Default, PartialEq, Debug)]
    struct Msg48 {
        seq: u64,
        a: u64,
        b: u64,
        c: u64,
        d: u64,
        e: u64,
    }

    let count: u64 = if cfg!(miri) { 100 } else { 10_000_000 };
    // 1 << 10 = 1024 slots; large enough for the test, small enough to avoid
    // stack overflow from the 48-byte * N inline storage in debug mode.
    let (tx, rx) = spsc_ring_copy::<Msg48, { 1 << 10 }>();

    let producer = thread::spawn(move || {
        for i in 0..count {
            let msg = Msg48 {
                seq: i,
                ..Msg48::default()
            };
            while !tx.push(&msg) {
                spin_loop();
            }
        }
    });

    let consumer = thread::spawn(move || {
        let mut out = Msg48::default();
        for i in 0..count {
            while !rx.pop(&mut out) {
                spin_loop();
            }
            assert_eq!(out.seq, i);
        }
    });

    #[expect(
        clippy::expect_used,
        reason = "test harness: panics are the correct failure mode"
    )]
    producer.join().expect("producer panicked");
    #[expect(
        clippy::expect_used,
        reason = "test harness: panics are the correct failure mode"
    )]
    consumer.join().expect("consumer panicked");
}

#[cfg(feature = "alloc")]
#[test]
fn heap_push_pop() {
    let mut ring = SpscRingCopyHeap::<u64>::with_capacity(128);
    assert!(ring.push(&42));
    let mut out = 0u64;
    assert!(ring.pop(&mut out));
    assert_eq!(out, 42);
}

#[test]
fn instrumented_counts() {
    let mut ring = SpscRingCopyInstrumented::<u64, 8>::new();
    ring.push(&1);
    ring.push(&2);
    let mut out = 0u64;
    ring.pop(&mut out);
    assert_eq!(ring.instrumentation().push_count(), 2);
    assert_eq!(ring.instrumentation().pop_count(), 1);
}

#[cfg(feature = "alloc")]
#[test]
fn split_handles_two_thread() {
    use core::hint::spin_loop;
    use mantis_queue::spsc_ring_copy;
    use std::thread;

    let count = if cfg!(miri) { 1_000u64 } else { 10_000u64 };
    let (tx, rx) = spsc_ring_copy::<u64, 1024>();

    let producer = thread::spawn(move || {
        for i in 0..count {
            while !tx.push(&i) {
                spin_loop();
            }
        }
    });

    let consumer = thread::spawn(move || {
        let mut out = 0u64;
        for i in 0..count {
            while !rx.pop(&mut out) {
                spin_loop();
            }
            assert_eq!(out, i);
        }
    });

    #[expect(
        clippy::expect_used,
        reason = "test harness: panics are the correct failure mode"
    )]
    producer.join().expect("producer panicked");
    #[expect(
        clippy::expect_used,
        reason = "test harness: panics are the correct failure mode"
    )]
    consumer.join().expect("consumer panicked");
}

#[cfg(feature = "alloc")]
#[test]
fn split_handles_batch_two_thread() {
    use core::hint::spin_loop;
    use mantis_queue::spsc_ring_copy;
    use std::thread;

    let total = if cfg!(miri) { 500u64 } else { 100_000u64 };
    let batch_size = 50usize;
    let (tx, rx) = spsc_ring_copy::<u64, 1024>();

    let producer = thread::spawn(move || {
        let mut sent = 0u64;
        while sent < total {
            let remaining = usize::try_from(total - sent).unwrap_or(usize::MAX);
            let n = remaining.min(batch_size);
            let batch: Vec<u64> = (sent..sent + n as u64).collect();
            let pushed = tx.push_batch(&batch);
            sent += pushed as u64;
            if pushed == 0 {
                spin_loop();
            }
        }
    });

    let consumer = thread::spawn(move || {
        let mut received = 0u64;
        let mut buf = vec![0u64; batch_size];
        while received < total {
            let popped = rx.pop_batch(&mut buf);
            for val in &buf[..popped] {
                assert_eq!(*val, received);
                received += 1;
            }
            if popped == 0 {
                spin_loop();
            }
        }
    });

    #[expect(
        clippy::expect_used,
        reason = "test harness: panics are the correct failure mode"
    )]
    producer.join().expect("producer panicked");
    #[expect(
        clippy::expect_used,
        reason = "test harness: panics are the correct failure mode"
    )]
    consumer.join().expect("consumer panicked");
}
