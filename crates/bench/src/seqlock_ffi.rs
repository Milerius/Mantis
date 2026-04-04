//! FFI bindings to `rigtorp::Seqlock` (C++ contender).
//!
//! Wraps the C functions from `cpp/seqlock_bench_contender.cpp` into safe Rust
//! types for use in benchmarks.

#![allow(unsafe_code, missing_docs, clippy::missing_panics_doc)]

/// 64-byte message type matching `BenchMsg64` in `seqlock_bench_contender.cpp`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BenchMsg64 {
    pub data: [u8; 64],
}

/// 128-byte message type matching `BenchMsg128` in `seqlock_bench_contender.cpp`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BenchMsg128 {
    pub data: [u8; 128],
}

unsafe extern "C" {
    // u64
    pub fn rigtorp_seqlock_write_u64(val: u64);
    pub fn rigtorp_seqlock_read_u64() -> u64;
    // 64-byte
    pub fn rigtorp_seqlock_write_64(val: *const BenchMsg64);
    pub fn rigtorp_seqlock_read_64(out: *mut BenchMsg64);
    // 128-byte
    pub fn rigtorp_seqlock_write_128(val: *const BenchMsg128);
    pub fn rigtorp_seqlock_read_128(out: *mut BenchMsg128);
}
