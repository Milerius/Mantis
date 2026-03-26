//! Performance counter abstraction.
//!
//! Provides a platform-neutral [`CycleCounter`] trait and [`Measurement`] struct.
//! The [`InstantCounter`] fallback is available with the `std` feature.
//!
//! Platform-specific counters (`RdtscCounter`, `KperfCounter`, `PmuCounter`)
//! will be wired as [`DefaultCounter`] in future tasks. For now, [`DefaultCounter`]
//! falls back to [`InstantCounter`] on all platforms.

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

/// Default counter selected at compile time (requires the `std` feature).
///
/// Platform-specific counters (`RdtscCounter`, `KperfCounter`, `PmuCounter`)
/// will be wired here in Tasks 15-16. For now, falls back to [`InstantCounter`].
#[cfg(feature = "std")]
pub type DefaultCounter = InstantCounter;
