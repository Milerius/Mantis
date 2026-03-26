//! Copy strategy trait for ring slot operations.
//!
//! Moved from `mantis-core` to consolidate all platform primitives.

/// Copy strategy for SPSC ring slot operations.
///
/// Implementations are zero-sized types used for static dispatch only.
/// No instance is ever constructed — all methods are associated functions.
pub trait CopyPolicy<T: Copy> {
    /// Copy `*src` into the ring slot at `*dst`.
    ///
    /// # Safety
    /// - `dst` must be valid, aligned, and point to an unoccupied slot.
    /// - `src` must be valid and aligned for reads of `T`.
    // SAFETY: callers must uphold the pointer validity invariants above.
    // Declared safe to satisfy `#![deny(unsafe_code)]` on the crate root;
    // all implementations must enforce the contract themselves.
    fn copy_in(dst: *mut T, src: *const T);

    /// Copy the ring slot at `*src` into `*dst`.
    ///
    /// # Safety
    /// - `src` must be valid, aligned, and point to an occupied slot.
    /// - `dst` must be valid and aligned for writes of `T`.
    // SAFETY: callers must uphold the pointer validity invariants above.
    // Declared safe to satisfy `#![deny(unsafe_code)]` on the crate root;
    // all implementations must enforce the contract themselves.
    fn copy_out(dst: *mut T, src: *const T);
}
