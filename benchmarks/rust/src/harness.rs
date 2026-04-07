//! Two-thread SPSC benchmark harness with core pinning and rdtsc timestamping.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use crate::message::{make_msg, Message48};
use crate::queues::{QueueBench, QueueConsumer, QueueProducer};
use crate::rdtsc::rdtsc_serialized;
use crate::stats::CycleHistogram;

/// Pin the current thread to the given logical core id.
fn pin_to_core(core_id: usize) {
    let core_ids = core_affinity::get_core_ids().expect("failed to get core ids");
    let target = core_ids
        .into_iter()
        .find(|c| c.id == core_id)
        .unwrap_or_else(|| panic!("core {core_id} not found"));
    core_affinity::set_for_current(target);
}

/// Run a two-thread SPSC latency benchmark.
///
/// The producer runs on the calling thread (pinned to `producer_core`),
/// the consumer runs on a spawned thread (pinned to `consumer_core`).
/// Returns the consumer-side latency histogram.
pub fn run_bench<Q>(
    queue: Q,
    producer_core: usize,
    consumer_core: usize,
    warmup: u64,
    messages: u64,
) -> CycleHistogram
where
    Q: QueueBench,
    Q::Producer: 'static,
    Q::Consumer: 'static,
{
    let total = warmup + messages;
    let (mut tx, rx) = queue.split();

    let consumer_ready = Arc::new(AtomicBool::new(false));
    let producer_ready = Arc::new(AtomicBool::new(false));

    let cr = Arc::clone(&consumer_ready);
    let pr = Arc::clone(&producer_ready);

    // Spawn consumer thread
    let consumer_handle = thread::spawn(move || {
        pin_to_core(consumer_core);

        let mut rx = rx;
        let mut histogram = CycleHistogram::new();
        let mut msg = Message48::default();
        let mut received: u64 = 0;

        // Signal consumer is ready
        cr.store(true, Ordering::Release);

        // Wait for producer to be ready
        while !pr.load(Ordering::Acquire) {
            std::hint::spin_loop();
        }

        // Consumer loop
        while received < total {
            if rx.try_pop(&mut msg) {
                if received >= warmup {
                    let now = rdtsc_serialized();
                    let delta = now.wrapping_sub(msg.timestamp);
                    histogram.record(delta);
                }
                received += 1;
            } else {
                std::hint::spin_loop();
            }
        }

        histogram
    });

    // Pin producer to its core
    pin_to_core(producer_core);

    // Wait for consumer to be ready
    while !consumer_ready.load(Ordering::Acquire) {
        std::hint::spin_loop();
    }

    // Signal producer is ready
    producer_ready.store(true, Ordering::Release);

    // Producer loop
    for i in 0..total {
        let mut msg = make_msg(i);
        msg.timestamp = rdtsc_serialized();

        while !tx.try_push(&msg) {
            std::hint::spin_loop();
        }
    }

    consumer_handle.join().expect("consumer thread panicked")
}
