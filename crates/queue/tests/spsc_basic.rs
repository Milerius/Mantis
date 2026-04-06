#![cfg(not(miri))]
//! Skip under Miri — queue uses inline assembly (cold_path, prefetch).
//! Integration tests for all SPSC ring presets.

use mantis_queue::{QueueError, SpscRing, SpscRingInstrumented};

#[cfg(feature = "alloc")]
use mantis_queue::{SpscRingHeap, spsc_ring};

#[test]
fn spsc_ring_fill_and_drain() {
    let mut ring = SpscRing::<u64, 8>::new();
    for i in 0..7 {
        assert!(ring.try_push(i).is_ok());
    }
    assert!(ring.try_push(99).is_err());
    for i in 0..7 {
        assert_eq!(ring.try_pop().ok(), Some(i));
    }
    assert_eq!(ring.try_pop(), Err(QueueError::Empty));
}

#[test]
fn spsc_ring_fill_and_drain_capacity_16() {
    let mut ring = SpscRing::<u64, 16>::new();
    for i in 0..15 {
        assert!(ring.try_push(i).is_ok());
    }
    for i in 0..15 {
        assert_eq!(ring.try_pop().ok(), Some(i));
    }
}

#[cfg(feature = "alloc")]
#[test]
fn heap_fill_and_drain() {
    let mut ring = SpscRingHeap::<u64>::with_capacity(8);
    for i in 0..7 {
        assert!(ring.try_push(i).is_ok());
    }
    for i in 0..7 {
        assert_eq!(ring.try_pop().ok(), Some(i));
    }
}

#[test]
fn instrumented_tracks_all_events() {
    let mut ring = SpscRingInstrumented::<u64, 4>::new();
    assert!(ring.try_push(1).is_ok());
    assert!(ring.try_push(2).is_ok());
    assert!(ring.try_push(3).is_ok());
    assert!(ring.try_push(4).is_err()); // full
    let _ = ring.try_pop();
    let _ = ring.try_pop();
    let _ = ring.try_pop();
    let _ = ring.try_pop(); // empty

    let instr = ring.instrumentation();
    assert_eq!(instr.push_count(), 3);
    assert_eq!(instr.pop_count(), 3);
    assert_eq!(instr.push_full_count(), 1);
    assert_eq!(instr.pop_empty_count(), 1);
}

/// Verify that T: Drop types are correctly dropped when the ring
/// is dropped with un-popped elements.
#[cfg(feature = "alloc")]
#[test]
fn drop_safety_with_string() {
    let mut ring = SpscRing::<String, 4>::new();
    ring.try_push(String::from("alpha")).ok();
    ring.try_push(String::from("beta")).ok();
    let _ = ring.try_pop();
    drop(ring);
    // If we get here without miri complaints, drop is correct.
}

#[test]
fn extensive_wraparound() {
    let mut ring = SpscRing::<u64, 4>::new();
    for round in 0..100 {
        for i in 0..3 {
            let val = round * 3 + i;
            assert!(ring.try_push(val).is_ok(), "push failed at {val}");
        }
        for i in 0..3 {
            let expected = round * 3 + i;
            assert_eq!(ring.try_pop().ok(), Some(expected));
        }
    }
}

#[cfg(feature = "alloc")]
#[test]
fn split_handles_two_thread() {
    use std::thread;

    let (mut tx, mut rx) = spsc_ring::<u64, 1024>();
    let count = 10_000u64;

    let producer = thread::spawn(move || {
        for i in 0..count {
            while tx.try_push(i).is_err() {
                core::hint::spin_loop();
            }
        }
    });

    let consumer = thread::spawn(move || {
        for i in 0..count {
            loop {
                match rx.try_pop() {
                    Ok(val) => {
                        assert_eq!(val, i, "FIFO violation");
                        break;
                    }
                    Err(_) => core::hint::spin_loop(),
                }
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
