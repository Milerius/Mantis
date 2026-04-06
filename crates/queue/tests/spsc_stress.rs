#![cfg(not(miri))]
//! Skip under Miri — queue uses inline assembly (`cold_path`, `prefetch`).
//! Two-thread stress test: 10M sequential u64 values.

#[cfg(feature = "alloc")]
#[test]
fn stress_10m_items() {
    use mantis_queue::spsc_ring;
    use std::thread;

    let item_count: u64 = if cfg!(miri) { 1_000 } else { 10_000_000 };

    let (mut tx, mut rx) = spsc_ring::<u64, 4096>();

    let producer = thread::spawn(move || {
        for i in 0..item_count {
            while tx.try_push(i).is_err() {
                core::hint::spin_loop();
            }
        }
    });

    let consumer = thread::spawn(move || {
        let mut expected = 0u64;
        while expected < item_count {
            match rx.try_pop() {
                Ok(val) => {
                    assert_eq!(
                        val, expected,
                        "FIFO violation: expected {expected}, got {val}"
                    );
                    expected += 1;
                }
                Err(_) => core::hint::spin_loop(),
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
