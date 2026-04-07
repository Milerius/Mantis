//! Two-thread SPSC benchmark harness with core pinning and rdtsc timestamping.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use crate::message::{make_msg, Message48};
use crate::queues::{QueueBench, QueueConsumer, QueueProducer};
use crate::rdtsc::rdtsc_serialized;
use crate::stats::CycleHistogram;

/// Pin the current thread to a specific logical core using `sched_setaffinity`.
///
/// Works on isolated cores (`isolcpus`) unlike `core_affinity` which only
/// sees cores in the process's default affinity mask.
#[cfg(target_os = "linux")]
fn pin_to_core(core_id: usize) {
    // SAFETY: cpu_set is zeroed, then we set exactly one bit for the target core.
    // sched_setaffinity(0, ...) targets the calling thread. The cpu_set lives
    // on the stack and is valid for the duration of the call.
    unsafe {
        let mut cpu_set: libc::cpu_set_t = std::mem::zeroed();
        libc::CPU_SET(core_id, &mut cpu_set);
        let ret = libc::sched_setaffinity(
            0, // 0 = calling thread
            std::mem::size_of::<libc::cpu_set_t>(),
            &cpu_set,
        );
        assert!(
            ret == 0,
            "sched_setaffinity failed for core {core_id}: errno {}",
            *libc::__errno_location()
        );
    }
}

#[cfg(not(target_os = "linux"))]
fn pin_to_core(_core_id: usize) {
    eprintln!("WARNING: core pinning not supported on this platform");
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
    assert_ne!(
        producer_core, consumer_core,
        "producer and consumer must be on different cores"
    );
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
