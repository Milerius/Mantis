//! ASM inspection shim — forces monomorphization of hot-path functions.
//!
//! Not a real example. Used by `scripts/check-asm.sh` to produce
//! inspectable assembly for `try_push`, `try_pop`, and variants.
//!
//! Usage: `cargo asm -p mantis-queue --example asm_shim <symbol>`

#![expect(
    missing_docs,
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "ASM inspection shim, not a real example"
)]

use mantis_queue::{SpscRing, SpscRingCopy};

#[inline(never)]
pub fn spsc_push_u64(ring: &mut SpscRing<u64, 1024>, val: u64) -> bool {
    ring.try_push(val).is_ok()
}

#[inline(never)]
pub fn spsc_pop_u64(ring: &mut SpscRing<u64, 1024>) -> Option<u64> {
    ring.try_pop().ok()
}

#[inline(never)]
pub fn spsc_push_bytes64(ring: &mut SpscRing<[u8; 64], 1024>, val: [u8; 64]) -> bool {
    ring.try_push(val).is_ok()
}

#[inline(never)]
pub fn spsc_pop_bytes64(ring: &mut SpscRing<[u8; 64], 1024>) -> Option<[u8; 64]> {
    ring.try_pop().ok()
}

#[inline(never)]
pub fn spsc_copy_push_u64(ring: &mut SpscRingCopy<u64, 1024>, val: &u64) -> bool {
    ring.push(val)
}

#[inline(never)]
pub fn spsc_copy_pop_u64(ring: &mut SpscRingCopy<u64, 1024>, out: &mut u64) -> bool {
    ring.pop(out)
}

fn main() {
    let mut ring = SpscRing::<u64, 1024>::new();
    std::hint::black_box(spsc_push_u64(&mut ring, 42));
    std::hint::black_box(spsc_pop_u64(&mut ring));

    let mut ring_bytes = SpscRing::<[u8; 64], 1024>::new();
    std::hint::black_box(spsc_push_bytes64(&mut ring_bytes, [0u8; 64]));
    std::hint::black_box(spsc_pop_bytes64(&mut ring_bytes));

    let mut copy_ring = SpscRingCopy::<u64, 1024>::new();
    std::hint::black_box(spsc_copy_push_u64(&mut copy_ring, &42));
    let mut copy_out = 0u64;
    std::hint::black_box(spsc_copy_pop_u64(&mut copy_ring, &mut copy_out));
}
