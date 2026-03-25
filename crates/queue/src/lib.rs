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

#![no_std]
#![deny(unsafe_code)]

#[cfg(feature = "std")]
extern crate std;

mod pad;
mod raw;
pub mod storage;

pub use mantis_core::{ImmediatePush, NoInstr, Pow2Masked};
pub use mantis_types::QueueError;
pub use pad::CachePadded;
#[cfg(feature = "alloc")]
pub use storage::HeapStorage;
pub use storage::{InlineStorage, Storage};
