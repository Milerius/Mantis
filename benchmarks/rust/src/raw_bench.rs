//! Zero-overhead SPSC benchmark matching HFT University protocol.
//!
//! Optimized ring with:
//! - 64-byte cache-line padding (x86_64 native, not 128-byte Apple)
//! - Colocated producer fields (head + tail_cached on same cache line)
//! - Colocated consumer fields (tail + head_cached on same cache line)
//! - Branch-based wrapping (branch predictor > always-execute AND)
//! - No Arc, no Result, bool API, sum += delta
//! - LTO + codegen-units=1 for guaranteed cross-crate inlining

#![allow(unsafe_code)]

use core::cell::Cell;
use core::mem::MaybeUninit;
use core::ptr;
use core::sync::atomic::{AtomicUsize, Ordering};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::thread;

// ─── Message ────────────────────────────────────────────────────────────────

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
        // SAFETY: all-zero is valid for this repr(C) POD struct.
        unsafe { core::mem::zeroed() }
    }
}

// ─── Optimized ring engine ──────────────────────────────────────────────────

const CAPACITY: usize = 1024;

/// Producer-local cache line: head atomic + cached tail.
/// Both accessed only by the producer thread — no false sharing.
#[repr(C, align(64))]
struct ProducerLine {
    head: AtomicUsize,
    tail_cached: Cell<usize>,
}

/// Consumer-local cache line: tail atomic + cached head.
/// Both accessed only by the consumer thread — no false sharing.
#[repr(C, align(64))]
struct ConsumerLine {
    tail: AtomicUsize,
    head_cached: Cell<usize>,
}

/// Minimal SPSC ring — colocated cache lines, 64-byte padding, branch wrap.
#[repr(C)]
struct RawRing {
    producer: ProducerLine,
    consumer: ConsumerLine,
    slots: [MaybeUninit<Msg>; CAPACITY],
}

// SAFETY: SPSC protocol — producer and consumer access disjoint fields.
unsafe impl Sync for RawRing {}

impl RawRing {
    fn new() -> Self {
        Self {
            producer: ProducerLine {
                head: AtomicUsize::new(0),
                tail_cached: Cell::new(0),
            },
            consumer: ConsumerLine {
                tail: AtomicUsize::new(0),
                head_cached: Cell::new(0),
            },
            // SAFETY: MaybeUninit doesn't require initialization
            slots: unsafe { MaybeUninit::uninit().assume_init() },
        }
    }

    /// Push a message. Returns true on success, false if full.
    #[inline(always)]
    fn push(&self, msg: Msg) -> bool {
        let head = self.producer.head.load(Ordering::Relaxed);

        // Branch-based wrap — branch predictor learns "never taken"
        let next = head + 1;
        let next = if next == CAPACITY { 0 } else { next };

        if next == self.producer.tail_cached.get() {
            let tail = self.consumer.tail.load(Ordering::Acquire);
            self.producer.tail_cached.set(tail);
            if next == tail {
                return false;
            }
        }

        // SAFETY: SPSC producer owns this slot exclusively
        unsafe {
            let slot = self.slots.as_ptr().add(head) as *mut Msg;
            ptr::write(slot, msg);
        }

        self.producer.head.store(next, Ordering::Release);
        true
    }

    /// Pop a message into `out`. Returns true on success, false if empty.
    #[inline(always)]
    fn pop(&self, out: *mut Msg) -> bool {
        let tail = self.consumer.tail.load(Ordering::Relaxed);

        if tail == self.consumer.head_cached.get() {
            let head = self.producer.head.load(Ordering::Acquire);
            self.consumer.head_cached.set(head);
            if tail == head {
                return false;
            }
        }

        // SAFETY: SPSC consumer owns this slot exclusively
        unsafe {
            let slot = self.slots.as_ptr().add(tail) as *const Msg;
            ptr::copy_nonoverlapping(slot, out, 1);
        }

        // Branch-based wrap
        let next = tail + 1;
        let next = if next == CAPACITY { 0 } else { next };
        self.consumer.tail.store(next, Ordering::Release);
        true
    }
}

// ─── Platform ────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn pin(core: usize) {
    // SAFETY: zeroed cpu_set, one bit set, targeting current thread.
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
    // SAFETY: x86_64 inline asm matching HFT University's rdtsc_fenced.
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
    let ring = Box::leak(Box::new(RawRing::new()));
    let ring_addr = ring as *const RawRing as usize;

    let consumer_ready = Box::leak(Box::new(AtomicBool::new(false)));
    let ready_addr = consumer_ready as *const AtomicBool as usize;

    let total_latency = Box::leak(Box::new(AtomicU64::new(0)));
    let latency_addr = total_latency as *const AtomicU64 as usize;

    // Consumer — spawned first, signals ready
    let consumer = thread::spawn(move || {
        pin(consumer_core);
        let ring = unsafe { &*(ring_addr as *const RawRing) };
        let ready = unsafe { &*(ready_addr as *const AtomicBool) };
        let latency = unsafe { &*(latency_addr as *const AtomicU64) };
        ready.store(true, Ordering::Release);

        let mut msg = Msg::default();
        let mut sum: u64 = 0;
        let mut count: u64 = 0;
        while count < total_ops {
            if ring.pop(&mut msg as *mut Msg) {
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
        let ring = unsafe { &*(ring_addr as *const RawRing) };
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
            while !ring.push(msg) {}
        }
    });

    producer.join().unwrap();
    consumer.join().unwrap();

    let sum = unsafe { &*(latency_addr as *const AtomicU64) }.load(Ordering::Acquire);

    // Reclaim
    unsafe {
        drop(Box::from_raw(ring_addr as *mut RawRing));
        drop(Box::from_raw(ready_addr as *mut AtomicBool));
        drop(Box::from_raw(latency_addr as *mut AtomicU64));
    }

    sum
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
