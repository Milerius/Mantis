//! Performance counter abstraction.
//!
//! Provides a platform-neutral [`CycleCounter`] trait and [`Measurement`] struct.
//! The [`InstantCounter`] fallback is available with the `std` feature.
//!
//! Platform-specific counters are wired as [`DefaultCounter`] based on the
//! current target:
//! - `x86_64` with `asm` + `std`: [`RdtscCounter`]
//! - macOS ARM64: [`KperfCounter`] (`mach_absolute_time`)
//! - Linux ARM64: [`PmuCounter`] (`clock_gettime(CLOCK_MONOTONIC)`)
//! - All others (with `std`): [`InstantCounter`]

#[cfg(feature = "std")]
mod instant;

#[cfg(feature = "std")]
pub use instant::InstantCounter;

/// A measurement from a performance counter.
#[derive(Debug, Clone, Copy)]
pub struct Measurement {
    /// Wall-clock duration in nanoseconds.
    pub nanos: u64,
    /// CPU cycles (if available on this platform, else 0).
    pub cycles: u64,
}

/// Trait for platform-specific cycle counting.
///
/// Implementations must be `Send + Sync` to allow sharing across threads
/// (e.g., a counter embedded in a benchmark harness passed to worker threads).
pub trait CycleCounter: Send + Sync {
    /// Start a measurement, returning an opaque timestamp.
    ///
    /// The returned value is an implementation-defined opaque timestamp.
    /// Pass it unchanged to [`elapsed`](Self::elapsed).
    fn start(&self) -> u64;

    /// End a measurement, returning elapsed time since `start`.
    ///
    /// `start` must be a value previously returned by [`start`](Self::start)
    /// on the same counter instance.
    fn elapsed(&self, start: u64) -> Measurement;
}

// DefaultCounter is selected at compile time (first match wins):
//   1. x86_64 + `asm` + `std`: RdtscCounter
//   2. macOS ARM64: KperfCounter
//   3. Linux ARM64: PmuCounter
//   4. Any platform with `std`: InstantCounter
cfg_if::cfg_if! {
    if #[cfg(all(target_arch = "x86_64", feature = "asm", feature = "std"))] {
        /// Default counter: RDTSC on x86_64 with `asm` + `std` features.
        pub type DefaultCounter = crate::isa_x86::rdtsc::RdtscCounter;
    } else if #[cfg(all(target_arch = "aarch64", target_os = "macos"))] {
        /// Default counter: `mach_absolute_time` on macOS ARM64.
        pub type DefaultCounter = crate::isa_arm64::counters::KperfCounter;
    } else if #[cfg(all(target_arch = "aarch64", target_os = "linux"))] {
        /// Default counter: `clock_gettime` on Linux ARM64.
        pub type DefaultCounter = crate::isa_arm64::counters::PmuCounter;
    } else if #[cfg(feature = "std")] {
        /// Default counter: `Instant`-based fallback.
        pub type DefaultCounter = InstantCounter;
    }
}

#[cfg(all(target_arch = "x86_64", feature = "asm", feature = "std"))]
pub use crate::isa_x86::rdtsc::RdtscCounter;

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
pub use crate::isa_arm64::counters::KperfCounter;

#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
pub use crate::isa_arm64::counters::PmuCounter;
