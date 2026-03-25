//! Copy-optimized SPSC ring buffer for `T: Copy` types.
//!
//! The copy-ring uses `CopyPolicy` for slot reads and writes, enabling
//! SIMD acceleration on `x86_64` (SSE2) and `aarch64` (NEON).
//!
//! The ring engine and public handles will be added in subsequent tasks.
pub(crate) mod raw;
