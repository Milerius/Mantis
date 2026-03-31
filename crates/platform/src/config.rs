//! Compile-time platform detection constants.

/// `true` when compiling for `x86_64`.
pub const X86_64: bool = cfg!(target_arch = "x86_64");

/// `true` when compiling for aarch64.
pub const AARCH64: bool = cfg!(target_arch = "aarch64");

/// `true` when targeting macOS.
pub const IS_MACOS: bool = cfg!(target_os = "macos");

/// `true` when targeting Linux.
pub const IS_LINUX: bool = cfg!(target_os = "linux");

/// Conservative cache-line size in bytes.
///
/// 128 covers both Intel (64B) and Apple Silicon (128B).
pub const CACHE_LINE: usize = 128;
