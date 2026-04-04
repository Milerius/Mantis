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

unsafe extern "C" {
    /// Write a value to the global rigtorp seqlock.
    pub fn rigtorp_seqlock_write_64(val: *const BenchMsg64);
    /// Read a value from the global rigtorp seqlock.
    pub fn rigtorp_seqlock_read_64(out: *mut BenchMsg64);
}
