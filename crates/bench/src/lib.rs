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

pub mod counters;
pub mod report;
