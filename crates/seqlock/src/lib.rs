//! Lock-free sequence lock for the Mantis SDK.
//!
//! A seqlock allows one writer thread to publish a value that multiple
//! reader threads can observe without blocking. The writer is never blocked.
//! Readers retry if they detect a concurrent write.
//!
//! # Architecture
//!
//! - Single writer enforced by `&mut self` — compile-time exclusivity
//! - Multiple readers via `&self` — `SeqLock<T>` is `Sync`
//! - Cache-line padded sequence counter prevents false sharing
//! - `CopyPolicy` strategy for pluggable SIMD copy optimization
//! - All unsafe code isolated in `raw` submodule
//!
//! This crate is `no_std` by default. Enable `std` for standard library support.

#![cfg_attr(feature = "nightly", feature(generic_const_exprs))]
#![cfg_attr(feature = "nightly", allow(incomplete_features))]
#![no_std]
#![deny(unsafe_code)]

mod raw;

pub use raw::seqlock::SeqLock;

use mantis_platform::DefaultCopyPolicy;
#[cfg(feature = "nightly")]
use mantis_platform::SimdCopyPolicy;

/// Default seqlock — portable, works everywhere.
pub type SeqLockDefault<T> = SeqLock<T, DefaultCopyPolicy>;

/// SIMD-optimized seqlock — NEON/SSE2 wide loads for faster reader copy.
/// Nightly only.
#[cfg(feature = "nightly")]
pub type SeqLockSimd<T> = SeqLock<T, SimdCopyPolicy>;
