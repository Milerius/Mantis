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

use mantis_platform::metering::{DefaultHwCounters, HwCounterDeltas, HwCounters};
use mantis_queue::SpscRing;

/// Result from a single benchmark run.
struct RunResult {
    total_cycles: u64,
    hw: Option<HwCounterDeltas>,
}

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

fn run_raw(producer_core: usize, consumer_core: usize, total_ops: u64) -> RunResult {
    let ring = SpscRing::<Msg, 1024>::new();
    run_raw_ring(&ring, producer_core, consumer_core, total_ops)
}

fn run_raw_fast(producer_core: usize, consumer_core: usize, total_ops: u64) -> RunResult {
    use mantis_queue::SpscRingFast;
    let ring = SpscRingFast::<Msg, 1024>::new();
    run_raw_ring(&ring, producer_core, consumer_core, total_ops)
}

/// Generic runner for any RawRing with push_shared/pop_shared.
fn run_raw_ring<S, I, P, Instr>(
    ring: &mantis_queue::RawRing<Msg, S, I, P, Instr>,
    producer_core: usize,
    consumer_core: usize,
    total_ops: u64,
) -> RunResult
where
    S: mantis_queue::Storage<Msg>,
    I: mantis_core::IndexStrategy,
    P: mantis_core::PushPolicy,
    Instr: mantis_core::Instrumentation + Sync,
{
    // Use &self shared references — no &mut aliasing UB.
    // SAFETY: SPSC protocol guarantees disjoint access. Ring lives on caller's
    // stack and we join both threads before returning.
    let ring_addr = ring as *const mantis_queue::RawRing<Msg, S, I, P, Instr> as usize;

    let consumer_ready = AtomicBool::new(false);
    let ready_addr = &consumer_ready as *const AtomicBool as usize;

    let total_latency = AtomicU64::new(0);
    let latency_addr = &total_latency as *const AtomicU64 as usize;

    // Storage for HW counter deltas — written by consumer, read after join.
    let mut hw_result: Option<HwCounterDeltas> = None;
    let hw_result_addr = &mut hw_result as *mut Option<HwCounterDeltas> as usize;

    // Consumer — spawned first
    let consumer = thread::spawn(move || {
        pin(consumer_core);
        let ready = unsafe { &*(ready_addr as *const AtomicBool) };
        let latency = unsafe { &*(latency_addr as *const AtomicU64) };

        // Try to set up HW counters on this (consumer) thread.
        let hw_counters = DefaultHwCounters::try_new().ok();

        ready.store(true, Ordering::Release);

        // Start HW counters before the hot loop.
        let snapshot = hw_counters.as_ref().and_then(|c| c.start());

        // SAFETY: SPSC consumer — only pops. &self avoids noalias interference.
        let rb = unsafe { &*(ring_addr as *const mantis_queue::RawRing<Msg, S, I, P, Instr>) };
        let mut msg = Msg::default();
        let mut sum: u64 = 0;
        let mut count: u64 = 0;
        while count < total_ops {
            if unsafe { rb.pop_shared(&mut msg as *mut Msg) } {
                let now = rdtsc();
                sum += now - msg.timestamp;
                count += 1;
            }
        }

        // Read HW counters after the hot loop.
        let deltas = hw_counters.as_ref().and_then(|c| c.read(&snapshot));

        latency.store(sum, Ordering::Release);

        // SAFETY: hw_result_addr points to stack variable in the caller frame.
        // We join this thread before reading hw_result, so no data race.
        unsafe {
            *(hw_result_addr as *mut Option<HwCounterDeltas>) = deltas;
        }
    });

    // Producer
    let producer = thread::spawn(move || {
        pin(producer_core);
        let ready = unsafe { &*(ready_addr as *const AtomicBool) };
        while !ready.load(Ordering::Acquire) {}

        // SAFETY: SPSC producer — only pushes. &self avoids noalias interference.
        let rb = unsafe { &*(ring_addr as *const mantis_queue::RawRing<Msg, S, I, P, Instr>) };
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
            while !unsafe { rb.push_shared(msg) } {}
        }
    });

    producer.join().unwrap();
    consumer.join().unwrap();

    RunResult {
        total_cycles: total_latency.load(Ordering::Acquire),
        hw: hw_result,
    }
}

// ─── SpscRingCopy variant ────────────────────────────────────────────────────

fn run_raw_copy(producer_core: usize, consumer_core: usize, total_ops: u64) -> RunResult {
    use mantis_queue::SpscRingCopy;

    let ring = SpscRingCopy::<Msg, 1024>::new();
    let ring_addr = &ring as *const SpscRingCopy<Msg, 1024> as usize;

    let consumer_ready = AtomicBool::new(false);
    let ready_addr = &consumer_ready as *const AtomicBool as usize;
    let total_latency = AtomicU64::new(0);
    let latency_addr = &total_latency as *const AtomicU64 as usize;
    let mut hw_result: Option<HwCounterDeltas> = None;
    let hw_result_addr = &mut hw_result as *mut Option<HwCounterDeltas> as usize;

    let consumer = thread::spawn(move || {
        pin(consumer_core);
        let ready = unsafe { &*(ready_addr as *const AtomicBool) };
        let latency = unsafe { &*(latency_addr as *const AtomicU64) };

        let hw_counters = DefaultHwCounters::try_new().ok();

        ready.store(true, Ordering::Release);

        let snapshot = hw_counters.as_ref().and_then(|c| c.start());

        let rb = unsafe { &*(ring_addr as *const SpscRingCopy<Msg, 1024>) };
        let mut msg = Msg::default();
        let mut sum: u64 = 0;
        let mut count: u64 = 0;
        while count < total_ops {
            if unsafe { rb.pop_shared(&mut msg) } {
                let now = rdtsc();
                sum += now - msg.timestamp;
                count += 1;
            }
        }

        let deltas = hw_counters.as_ref().and_then(|c| c.read(&snapshot));
        latency.store(sum, Ordering::Release);

        // SAFETY: hw_result_addr points to caller's stack, joined before read.
        unsafe {
            *(hw_result_addr as *mut Option<HwCounterDeltas>) = deltas;
        }
    });

    let producer = thread::spawn(move || {
        pin(producer_core);
        let ready = unsafe { &*(ready_addr as *const AtomicBool) };
        while !ready.load(Ordering::Acquire) {}

        let rb = unsafe { &*(ring_addr as *const SpscRingCopy<Msg, 1024>) };
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
            while !unsafe { rb.push_shared(&msg) } {}
        }
    });

    producer.join().unwrap();
    consumer.join().unwrap();

    RunResult {
        total_cycles: total_latency.load(Ordering::Acquire),
        hw: hw_result,
    }
}

// ─── rtrb variant ────────────────────────────────────────────────────────────

fn run_raw_rtrb(producer_core: usize, consumer_core: usize, total_ops: u64) -> RunResult {
    let (mut tx, mut rx) = rtrb::RingBuffer::<Msg>::new(1024);

    let consumer_ready = AtomicBool::new(false);
    let ready_addr = &consumer_ready as *const AtomicBool as usize;
    let total_latency = AtomicU64::new(0);
    let latency_addr = &total_latency as *const AtomicU64 as usize;
    let mut hw_result: Option<HwCounterDeltas> = None;
    let hw_result_addr = &mut hw_result as *mut Option<HwCounterDeltas> as usize;

    let consumer = thread::spawn(move || {
        pin(consumer_core);
        let ready = unsafe { &*(ready_addr as *const AtomicBool) };
        let latency = unsafe { &*(latency_addr as *const AtomicU64) };

        let hw_counters = DefaultHwCounters::try_new().ok();

        ready.store(true, Ordering::Release);

        let snapshot = hw_counters.as_ref().and_then(|c| c.start());

        let mut sum: u64 = 0;
        let mut count: u64 = 0;
        while count < total_ops {
            if let Ok(msg) = rx.pop() {
                let now = rdtsc();
                sum += now - msg.timestamp;
                count += 1;
            }
        }

        let deltas = hw_counters.as_ref().and_then(|c| c.read(&snapshot));
        latency.store(sum, Ordering::Release);

        // SAFETY: hw_result_addr points to caller's stack, joined before read.
        unsafe {
            *(hw_result_addr as *mut Option<HwCounterDeltas>) = deltas;
        }
    });

    let producer = thread::spawn(move || {
        pin(producer_core);
        let ready = unsafe { &*(ready_addr as *const AtomicBool) };
        while !ready.load(Ordering::Acquire) {}

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
            while tx.push(msg).is_err() {}
        }
    });

    producer.join().unwrap();
    consumer.join().unwrap();

    RunResult {
        total_cycles: total_latency.load(Ordering::Acquire),
        hw: hw_result,
    }
}

// ─── Runner ──────────────────────────────────────────────────────────────────

/// Format a per-op HW counter value (total / ops).
fn fmt_per_op(total: u64, ops: u64) -> String {
    let v = total as f64 / ops as f64;
    if v < 0.05 {
        format!("{v:.2}")
    } else {
        format!("{v:.1}")
    }
}

fn run_variant(
    name: &str,
    run_fn: fn(usize, usize, u64) -> RunResult,
    producer_core: usize,
    consumer_core: usize,
    ops: u64,
    iterations: usize,
) {
    eprintln!("[{name}]");
    // Warmup
    let _ = run_fn(producer_core, consumer_core, ops);

    let mut best = u64::MAX;
    for i in 1..=iterations {
        let result = run_fn(producer_core, consumer_core, ops);
        let cycles_per_op = result.total_cycles as f64 / ops as f64;
        if result.total_cycles < best {
            best = result.total_cycles;
        }
        if let Some(hw) = result.hw {
            eprintln!(
                "  run {i}/{iterations}: {cycles_per_op:.1} cycles/op | insns={} bmiss={} l1d={} llc={}",
                fmt_per_op(hw.instructions, ops),
                fmt_per_op(hw.branch_misses, ops),
                fmt_per_op(hw.l1d_misses, ops),
                fmt_per_op(hw.llc_misses, ops),
            );
        } else {
            eprintln!("  run {i}/{iterations}: {cycles_per_op:.1} cycles/op");
        }
    }
    let best_per_op = best as f64 / ops as f64;
    eprintln!("  BEST: {best_per_op:.1} cycles/op");
}

/// Run all variants or a specific one.
pub fn run_raw_bench(producer_core: usize, consumer_core: usize, ops: u64, iterations: usize) {
    run_variant(
        "mantis-inline Pow2Masked",
        run_raw,
        producer_core,
        consumer_core,
        ops,
        iterations,
    );
    eprintln!();
    run_variant(
        "mantis-inline BranchWrap",
        run_raw_fast,
        producer_core,
        consumer_core,
        ops,
        iterations,
    );
    eprintln!();
    run_variant(
        "mantis-copy",
        run_raw_copy,
        producer_core,
        consumer_core,
        ops,
        iterations,
    );
    eprintln!();
    run_variant(
        "rtrb (push/pop Result)",
        run_raw_rtrb,
        producer_core,
        consumer_core,
        ops,
        iterations,
    );
}
