//! Benchmark harness and performance counter utilities for the Mantis SDK.
//!
//! Provides:
//! - Criterion integration for statistical benchmarking
//! - Platform-aware performance counter collection (RDTSC+lfence on `x86_64`,
//!   monotonic time on ARM64)
//! - JSON/CSV export for cross-hardware comparison
//! - Warmup utilities for CPU frequency stabilization
//!
//! This is a `std`-only tooling crate.

#![deny(unsafe_code)]

pub mod bench_runner;
pub mod measurement;
pub mod messages;
pub mod report;
pub mod workloads;

#[cfg(feature = "bench-contenders-cpp")]
pub mod rigtorp_ffi;

pub use mantis_platform::metering::{CycleCounter, DefaultCounter, InstantCounter, Measurement};
