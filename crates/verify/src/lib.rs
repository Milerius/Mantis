//! Formal verification and property-based testing for the Mantis SDK.
//!
//! Contains kani proof harnesses, bolero property tests,
//! and differential testing utilities.

#![deny(unsafe_code)]

mod decoder_props;
mod fixed_props;
pub mod market_state_props;
pub mod seqlock_proofs;
pub mod seqlock_props;
mod spsc_diff;
#[cfg(kani)]
mod spsc_proofs;
mod spsc_props;
