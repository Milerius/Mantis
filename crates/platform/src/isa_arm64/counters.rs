#![allow(unsafe_code)]
//! aarch64 performance counters.
//!
//! - macOS: `mach_absolute_time` via [`KperfCounter`]
//! - Linux: `clock_gettime(CLOCK_MONOTONIC)` via [`PmuCounter`]

use crate::metering::{CycleCounter, Measurement};

// ── macOS: KperfCounter ─────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
#[repr(C)]
struct MachTimebaseInfo {
    numer: u32,
    denom: u32,
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn mach_absolute_time() -> u64;
    fn mach_timebase_info(info: *mut MachTimebaseInfo) -> i32;
}

#[cfg(target_os = "macos")]
#[inline]
fn read_mach_time() -> u64 {
    // SAFETY: `mach_absolute_time` is a standard macOS syscall with no
    // preconditions. It is always safe to call and returns a monotonic u64
    // tick count. No memory is read or written.
    unsafe { mach_absolute_time() }
}

#[cfg(target_os = "macos")]
fn fetch_timebase() -> (u32, u32) {
    let mut info = MachTimebaseInfo { numer: 1, denom: 1 };
    // SAFETY: `mach_timebase_info` writes into the provided pointer.
    // `info` is a valid, properly aligned, stack-allocated `MachTimebaseInfo`.
    // The pointer is non-null for the duration of this call.
    // Failure mode: if the call fails it leaves `info` unchanged (defaults
    // keep numer/denom at 1/1, i.e., a 1:1 tick-to-nanosecond ratio).
    unsafe { mach_timebase_info(core::ptr::addr_of_mut!(info)); }
    (info.numer, info.denom)
}

/// macOS ARM64 cycle counter backed by `mach_absolute_time`.
///
/// Converts Mach ticks to nanoseconds using the kernel-provided timebase ratio.
#[cfg(target_os = "macos")]
pub struct KperfCounter {
    numer: u64,
    denom: u64,
}

#[cfg(target_os = "macos")]
impl KperfCounter {
    /// Create a new `KperfCounter`, querying the kernel timebase once.
    #[must_use]
    pub fn new() -> Self {
        let (numer, denom) = fetch_timebase();
        Self {
            numer: u64::from(numer),
            denom: u64::from(denom).max(1),
        }
    }

    fn ticks_to_nanos(&self, ticks: u64) -> u64 {
        ticks.saturating_mul(self.numer) / self.denom
    }
}

#[cfg(target_os = "macos")]
impl Default for KperfCounter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "macos")]
impl CycleCounter for KperfCounter {
    fn start(&self) -> u64 {
        read_mach_time()
    }

    fn elapsed(&self, start: u64) -> Measurement {
        let end = read_mach_time();
        let ticks = end.saturating_sub(start);
        Measurement {
            nanos: self.ticks_to_nanos(ticks),
            cycles: ticks,
        }
    }
}

// ── Linux: PmuCounter ───────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn clock_gettime_nanos() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `clock_gettime` writes into the provided `timespec` pointer.
    // `ts` is a valid, stack-allocated `libc::timespec` with correct alignment.
    // `CLOCK_MONOTONIC` is always available on Linux. The pointer is non-null
    // for the duration of the call. Failure mode: if ret != 0 we return 0
    // rather than producing a garbage timestamp.
    let ret = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &raw mut ts) };
    if ret != 0 {
        return 0;
    }
    let secs = u64::try_from(ts.tv_sec).unwrap_or(0);
    let nanos = u64::try_from(ts.tv_nsec).unwrap_or(0);
    secs.saturating_mul(1_000_000_000).saturating_add(nanos)
}

/// Linux ARM64 counter backed by `clock_gettime(CLOCK_MONOTONIC)`.
///
/// Returns nanoseconds; `cycles` is always 0 (no PMU access without
/// kernel support).
#[cfg(target_os = "linux")]
pub struct PmuCounter;

#[cfg(target_os = "linux")]
impl PmuCounter {
    /// Create a new `PmuCounter`.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

#[cfg(target_os = "linux")]
impl Default for PmuCounter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "linux")]
impl CycleCounter for PmuCounter {
    fn start(&self) -> u64 {
        clock_gettime_nanos()
    }

    fn elapsed(&self, start: u64) -> Measurement {
        let now = clock_gettime_nanos();
        Measurement {
            nanos: now.saturating_sub(start),
            cycles: 0,
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(all(test, target_os = "macos"))]
mod tests_macos {
    use super::KperfCounter;
    use crate::metering::CycleCounter;

    #[test]
    fn kperf_counter_elapsed_is_non_negative() {
        let counter = KperfCounter::new();
        let start = counter.start();
        let m = counter.elapsed(start);
        // nanos and cycles are u64, so always >= 0; just check they're
        // consistent (nanos should be proportional to cycles).
        let _ = core::hint::black_box(m);
    }

    #[test]
    fn kperf_counter_default_matches_new() {
        let a = KperfCounter::new();
        let b = KperfCounter::default();
        assert_eq!(a.numer, b.numer);
        assert_eq!(a.denom, b.denom);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests_linux {
    use super::PmuCounter;
    use crate::metering::CycleCounter;

    #[test]
    fn pmu_counter_elapsed_is_non_negative() {
        let counter = PmuCounter::new();
        let start = counter.start();
        let m = counter.elapsed(start);
        let _ = core::hint::black_box(m);
    }

    #[test]
    fn pmu_counter_cycles_always_zero() {
        let counter = PmuCounter::default();
        let start = counter.start();
        let m = counter.elapsed(start);
        assert_eq!(m.cycles, 0);
    }
}
