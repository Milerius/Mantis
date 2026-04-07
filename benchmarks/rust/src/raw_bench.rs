//! Zero-overhead SPSC benchmark using mantis-queue library's bool API.
//!
//! Uses SpscRing::push/pop_into directly — the library now has:
//! - Colocated cache lines (head+tail_cached on same 64B line)
//! - Bool return (no Result overhead)
//! - inline(always) on hot paths
//!
//! Protocol matches HFT University run_latency() exactly:
//! sum += delta, no Vec, no histogram in hot loop.

#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;

use mantis_queue::SpscRing;

/// 48-byte message matching HFT University's `hftu::Message`.
#[repr(C, align(16))]
#[derive(Clone, Copy)]
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

impl Default for Msg {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

// ─── Platform ────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn pin(core: usize) {
    unsafe {
        let mut set: libc::cpu_set_t = core::mem::zeroed();
        libc::CPU_SET(core, &mut set);
        let rc = libc::sched_setaffinity(0, core::mem::size_of::<libc::cpu_set_t>(), &set);
        assert!(rc == 0, "pin to core {core} failed");
    }
}

#[cfg(not(target_os = "linux"))]
fn pin(_core: usize) {}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn rdtsc() -> u64 {
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

// ─── Benchmark ──────────────────────────────────────────────────────────────

fn run_raw(producer_core: usize, consumer_core: usize, total_ops: u64) -> u64 {
    // Use the library's SpscRing with the new bool API.
    // SpscRing uses CacheLine-colocated fields internally.
    let mut ring = SpscRing::<Msg, 1024>::new();

    // Cast to usize for Send across threads.
    // SAFETY: ring lives on this stack frame and we join both threads before returning.
    // SPSC protocol guarantees disjoint access (producer: push, consumer: pop_into).
    let ring_addr = &mut ring as *mut SpscRing<Msg, 1024> as usize;

    let consumer_ready = AtomicBool::new(false);
    let ready_addr = &consumer_ready as *const AtomicBool as usize;

    let total_latency = AtomicU64::new(0);
    let latency_addr = &total_latency as *const AtomicU64 as usize;

    // Consumer — spawned first
    let consumer = thread::spawn(move || {
        pin(consumer_core);
        let ready = unsafe { &*(ready_addr as *const AtomicBool) };
        let latency = unsafe { &*(latency_addr as *const AtomicU64) };
        ready.store(true, Ordering::Release);

        let rb = unsafe { &mut *(ring_addr as *mut SpscRing<Msg, 1024>) };
        let mut msg = Msg::default();
        let mut sum: u64 = 0;
        let mut count: u64 = 0;
        while count < total_ops {
            if unsafe { rb.pop_into(&mut msg as *mut Msg) } {
                let now = rdtsc();
                sum += now - msg.timestamp;
                count += 1;
            }
        }
        latency.store(sum, Ordering::Release);
    });

    // Producer
    let producer = thread::spawn(move || {
        pin(producer_core);
        let ready = unsafe { &*(ready_addr as *const AtomicBool) };
        while !ready.load(Ordering::Acquire) {}

        let rb = unsafe { &mut *(ring_addr as *mut SpscRing<Msg, 1024>) };
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
            while !rb.push(msg) {}
        }
    });

    producer.join().unwrap();
    consumer.join().unwrap();

    total_latency.load(Ordering::Acquire)
}

/// Run multiple iterations and print cycles/op.
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
