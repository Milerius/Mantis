//! Raw SPSC benchmark matching HFT University protocol exactly.
//!
//! Zero overhead where possible: no traits, no Vec, no histogram in hot loop.
//! Uses split handles (Arc-based) to avoid aliasing UB.
//! Just `sum += delta` in the consumer loop, like the reference.

#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use mantis_queue::spsc_ring;

/// 48-byte message matching HFT University's `hftu::Message`.
#[repr(C, align(16))]
#[derive(Clone, Copy, Default)]
pub struct Msg {
    pub timestamp: u64,
    pub sequence: u64,
    pub symbol_id: u32,
    pub side: u16,
    pub _pad: u16,
    pub price: i64,
    pub quantity: i64,
    pub order_id: i64,
}

const _: () = assert!(core::mem::size_of::<Msg>() == 48);

#[cfg(target_os = "linux")]
fn pin(core: usize) {
    // SAFETY: zeroed cpu_set, one bit set, targeting current thread.
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        libc::CPU_SET(core, &mut set);
        let rc = libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set);
        assert!(rc == 0, "pin to core {core} failed");
    }
}

#[cfg(not(target_os = "linux"))]
fn pin(_core: usize) {}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn rdtsc() -> u64 {
    // SAFETY: x86_64-only asm guarded by cfg. lfence serializes prior ops.
    // Matches HFT University's rdtsc_fenced exactly.
    unsafe {
        let lo: u32;
        let hi: u32;
        core::arch::asm!(
            "lfence",
            "rdtsc",
            out("eax") lo,
            out("edx") hi,
            options(nostack, nomem),
        );
        ((hi as u64) << 32) | lo as u64
    }
}

#[cfg(not(target_arch = "x86_64"))]
#[inline(always)]
fn rdtsc() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

/// Run the benchmark. Returns total_cycles (sum of all deltas).
///
/// Protocol matches HFT University `run_latency()`:
/// - Consumer spawned first, signals ready
/// - Producer spawned after consumer ready
/// - Consumer: `sum += rdtsc() - msg.timestamp` per pop
/// - Returns total sum
fn run_raw(producer_core: usize, consumer_core: usize, total_ops: u64) -> u64 {
    // Split handles — Arc-based, no aliasing UB
    let (mut tx, mut rx) = spsc_ring::<Msg, 1024>();

    let consumer_ready = AtomicBool::new(false);
    let ready_addr = &consumer_ready as *const AtomicBool as usize;

    // Consumer thread
    let consumer = thread::spawn(move || {
        pin(consumer_core);
        // SAFETY: ready_addr points to stack-local AtomicBool that outlives
        // this thread (we join before run_raw returns).
        unsafe { &*(ready_addr as *const AtomicBool) }.store(true, Ordering::Release);

        let mut sum: u64 = 0;
        let mut count: u64 = 0;
        while count < total_ops {
            if let Ok(msg) = rx.try_pop() {
                let now = rdtsc();
                sum += now - msg.timestamp;
                count += 1;
            }
        }
        sum
    });

    // Producer thread
    let producer = thread::spawn(move || {
        pin(producer_core);
        // Wait for consumer
        while !unsafe { &*(ready_addr as *const AtomicBool) }.load(Ordering::Acquire) {}

        for i in 0..total_ops {
            let msg = Msg {
                timestamp: rdtsc(),
                sequence: i,
                symbol_id: (i & 0xFFF) as u32,
                side: (i & 1) as u16,
                _pad: 0,
                price: (i * 100 + 1) as i64,
                quantity: ((i & 0xFF) + 1) as i64,
                order_id: i as i64,
            };
            while tx.try_push(msg).is_err() {}
        }
    });

    producer.join().unwrap();
    consumer.join().unwrap()
}

/// Run multiple iterations and print cycles/op (matching HFT University output).
pub fn run_raw_bench(producer_core: usize, consumer_core: usize, ops: u64, iterations: usize) {
    // Warmup
    let _ = run_raw(producer_core, consumer_core, ops);

    let mut best = u64::MAX;
    for i in 1..=iterations {
        let total_cycles = run_raw(producer_core, consumer_core, ops);
        let cycles_per_op = total_cycles as f64 / ops as f64;
        if total_cycles < best {
            best = total_cycles;
        }
        eprintln!("  run {i}/{iterations}: {cycles_per_op:.1} cycles/op");
    }
    let best_per_op = best as f64 / ops as f64;
    eprintln!("  BEST: {best_per_op:.1} cycles/op");
}
