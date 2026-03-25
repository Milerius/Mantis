//! Lock-free queue primitives for the Mantis SDK.
//!
//! This crate provides SPSC (single-producer, single-consumer) ring buffers
//! and other bounded queue implementations optimized for low-latency
//! financial systems.
//!
//! # Architecture
//!
//! Each queue primitive follows the modular strategy pattern:
//! - Generic internal engine parameterized by strategy traits
//! - Curated preset type aliases for common configurations
//! - Platform-specific fast paths via `cfg`-gated assembly
//! - All unsafe code isolated in `raw` submodules
//!
//! This crate is `no_std` by default. Enable the `std` feature for
//! standard library support.

#![cfg_attr(feature = "nightly", feature(generic_const_exprs, likely_unlikely))]
#![cfg_attr(feature = "nightly", allow(incomplete_features))]
#![no_std]
#![deny(unsafe_code)]

#[cfg(feature = "std")]
extern crate std;

pub(crate) mod copy_ring;
pub(crate) mod engine;
mod handle;
mod pad;
mod presets;
mod raw;
pub mod storage;

pub use handle::{Consumer, Producer, RawRing};
pub use presets::*;
#[cfg(feature = "alloc")]
pub use handle::{spsc_ring, spsc_ring_heap};
pub use mantis_core::{CountingInstr, ImmediatePush, NoInstr, Pow2Masked};
pub use mantis_types::{PushError, QueueError};
pub use pad::CachePadded;
#[cfg(feature = "alloc")]
pub use storage::HeapStorage;
pub use storage::{InlineStorage, Storage};
