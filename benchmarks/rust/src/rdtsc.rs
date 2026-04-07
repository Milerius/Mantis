//! Serialized rdtsc for x86_64 cycle-accurate timestamping.

/// Read TSC with serialization (lfence before rdtsc).
#[cfg(target_arch = "x86_64")]
#[inline]
pub fn rdtsc_serialized() -> u64 {
    unsafe {
        core::arch::x86_64::_mm_lfence();
        core::arch::x86_64::_rdtsc()
    }
}

#[cfg(not(target_arch = "x86_64"))]
#[inline]
pub fn rdtsc_serialized() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}
