//! Platform abstractions for the Mantis SDK.
//!
//! Consolidates all platform-specific code: constant-time types, compile-time
//! ISA detection, SIMD copy kernels, cycle counters, cache-line padding,
//! extended precision arithmetic, and bit operations. Full parity with
//! Constantine's `platforms/` module.
//!
//! This crate is `no_std` by default. Enable `std` for `InstantCounter`,
//! CPUID detection, and CPU name. Enable `asm` for `RdtscCounter` on `x86_64`.

#![no_std]
#![deny(unsafe_code)]
#![cfg_attr(feature = "nightly", feature(generic_const_exprs))]
#![cfg_attr(feature = "nightly", allow(incomplete_features))]

#[cfg(feature = "std")]
extern crate std;

pub mod bithacks;
pub mod config;
pub mod constant_time;
pub mod pad;

// Top-level re-exports for convenience
pub use constant_time::{Borrow, CTBool, Carry, Ct};
pub use constant_time::{ccopy, ccopy32, ccopy_usize, mux, mux32, mux_bool, mux_bool32,
    mux_bool_usize, mux_usize, secret_lookup};
pub use constant_time::{div2n1n, div2n1n_u32};
pub use pad::CachePadded;
