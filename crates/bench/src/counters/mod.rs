//! Platform-aware performance counter collection.
//!
//! # `x86_64` (with `asm` feature)
//!
//! Uses `rdtsc` with `lfence` serializing barrier for accurate cycle counts.
//! The `lfence` ensures all prior instructions complete before reading the TSC.
//!
//! # `aarch64` macOS
//!
//! Uses `mach_absolute_time()` converted to nanoseconds via `mach_timebase_info`.
//!
//! # `aarch64` Linux
//!
//! Uses `clock_gettime(CLOCK_MONOTONIC)` via libc.
//!
//! # Fallback
//!
//! All other platforms use `std::time::Instant`.

mod instant;

#[cfg(all(target_arch = "x86_64", feature = "asm"))]
mod rdtsc;

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
mod kperf;

#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
mod pmu;

pub use instant::InstantCounter;

#[cfg(all(target_arch = "x86_64", feature = "asm"))]
pub use rdtsc::RdtscCounter;

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
pub use kperf::KperfCounter;

#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
pub use pmu::PmuCounter;

/// A measurement from a performance counter.
#[derive(Debug, Clone, Copy)]
pub struct Measurement {
    /// Wall-clock duration in nanoseconds.
    pub nanos: u64,
    /// CPU cycles (if available on this platform, else 0).
    pub cycles: u64,
}

/// Trait for platform-specific cycle counting.
pub trait CycleCounter: Send + Sync {
    /// Start a measurement.
    fn start(&self) -> u64;
    /// End a measurement, returning elapsed.
    fn elapsed(&self, start: u64) -> Measurement;
}

/// Default counter selected at compile time based on target platform.
///
/// - `x86_64` + `asm` feature: `RdtscCounter`
/// - `aarch64` macOS: `KperfCounter`
/// - `aarch64` Linux: `PmuCounter`
/// - All others: `InstantCounter`
#[cfg(all(target_arch = "x86_64", feature = "asm"))]
pub type DefaultCounter = RdtscCounter;

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
/// Default counter for macOS ARM64: `mach_absolute_time`-based.
pub type DefaultCounter = KperfCounter;

#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
pub type DefaultCounter = PmuCounter;

#[cfg(not(any(
    all(target_arch = "x86_64", feature = "asm"),
    all(target_arch = "aarch64", target_os = "macos"),
    all(target_arch = "aarch64", target_os = "linux"),
)))]
pub type DefaultCounter = InstantCounter;
