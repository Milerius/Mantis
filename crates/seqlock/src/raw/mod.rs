//! Unsafe internals for the sequence lock.
//!
//! All unsafe code in `mantis-seqlock` lives in this module.
//! The crate root denies unsafe; this module explicitly allows it.

#![expect(
    unsafe_code,
    reason = "seqlock core requires unsafe for atomic fences and volatile reads"
)]

pub(crate) mod seqlock;
