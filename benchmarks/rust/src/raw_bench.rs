//! Zero-overhead SPSC benchmark matching HFT University protocol.
//!
//! Bypasses all Rust API overhead:
//! - No Arc (stack-local engine, raw pointer field projections)
//! - No Result (inline push/pop returning bool)
//! - No &mut self (raw pointer access, same as C++ does)
//! - No traits, no Vec, no histogram in hot loop
//! - sum += delta like HFT University reference

#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;

use core::cell::Cell;
use core::mem::MaybeUninit;
use core::ptr;

use mantis_platform::CachePadded;

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

// ─── Minimal ring engine (inlined, no generics, no Result) ──────────────────

const CAPACITY: usize = 1024;
const MASK: usize = CAPACITY - 1;

/// Minimal SPSC ring — same algorithm as RingEngine but with bool returns
/// and no generic parameters. Stack-allocated, zero indirection.
#[repr(C)]
struct RawRing {
    head: CachePadded<AtomicUsize>,
    tail: CachePadded<AtomicUsize>,
    // Producer-local: cached tail (avoids cross-core Acquire on hot path)
    tail_cached: CachePadded<Cell<usize>>,
    // Consumer-local: cached head
    head_cached: CachePadded<Cell<usize>>,
    // Slot buffer
    slots: [MaybeUninit<Msg>; CAPACITY],
}

// SAFETY: SPSC protocol — producer and consumer access disjoint fields.
// Producer: head (write), tail_cached (read/write local), slots[head] (write)
// Consumer: tail (write), head_cached (read/write local), slots[tail] (read)
unsafe impl Sync for RawRing {}

impl RawRing {
    fn new() -> Self {
        Self {
            head: CachePadded::new(AtomicUsize::new(0)),
            tail: CachePadded::new(AtomicUsize::new(0)),
            tail_cached: CachePadded::new(Cell::new(0)),
            head_cached: CachePadded::new(Cell::new(0)),
            // SAFETY: MaybeUninit doesn't require initialization
            slots: unsafe { MaybeUninit::uninit().assume_init() },
        }
    }

    /// Push a message. Returns true on success, false if full.
    /// Caller must be the SOLE producer thread.
    #[inline(always)]
    fn push(&self, msg: Msg) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let next = (head + 1) & MASK;

        if next == self.tail_cached.get() {
            let tail = self.tail.load(Ordering::Acquire);
            self.tail_cached.set(tail);
            if next == tail {
                return false;
            }
        }

        // SAFETY: SPSC producer owns this slot exclusively
        unsafe {
            let slot = self.slots.as_ptr().add(head) as *mut Msg;
            ptr::write(slot, msg);
        }

        self.head.store(next, Ordering::Release);
        true
    }

    /// Pop a message into `out`. Returns true on success, false if empty.
    /// Caller must be the SOLE consumer thread.
    #[inline(always)]
    fn pop(&self, out: &mut Msg) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);

        if tail == self.head_cached.get() {
            let head = self.head.load(Ordering::Acquire);
            self.head_cached.set(head);
            if tail == head {
                return false;
            }
        }

        // SAFETY: SPSC consumer owns this slot exclusively
        unsafe {
            let slot = self.slots.as_ptr().add(tail) as *const Msg;
            ptr::copy_nonoverlapping(slot, out, 1);
        }

        let next = (tail + 1) & MASK;
        self.tail.store(next, Ordering::Release);
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

/// Run benchmark returning total latency (sum of all rdtsc deltas).
/// Protocol matches HFT University run_latency() exactly.
fn run_raw(producer_core: usize, consumer_core: usize, total_ops: u64) -> u64 {
    // Stack-local ring — Box to avoid stack overflow, then leak for 'static
    let ring = Box::leak(Box::new(RawRing::new()));
    let ring_addr = ring as *const RawRing as usize;

    let consumer_ready = Box::leak(Box::new(AtomicBool::new(false)));
    let ready_addr = consumer_ready as *const AtomicBool as usize;

    // Consumer thread — spawned first, signals ready
    let consumer = thread::spawn(move || {
        pin(consumer_core);
        let ring = unsafe { &*(ring_addr as *const RawRing) };
        let ready = unsafe { &*(ready_addr as *const AtomicBool) };
        ready.store(true, Ordering::Release);

        let mut msg = Msg::default();
        let mut sum: u64 = 0;
        let mut count: u64 = 0;
        while count < total_ops {
            if ring.pop(&mut msg) {
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
    let sum = consumer.join().unwrap();

    // Reclaim leaked memory
    unsafe {
        drop(Box::from_raw(ring_addr as *mut RawRing));
        drop(Box::from_raw(ready_addr as *mut AtomicBool));
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
