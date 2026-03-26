//! `x86_64` platform support.

pub mod assembler;
pub mod simd;

#[cfg(all(feature = "asm", feature = "std"))]
pub mod rdtsc;

#[cfg(feature = "std")]
pub mod cpudetect;
