//! Formal verification and property-based testing for the Mantis SDK.
//!
//! Contains kani proof harnesses, bolero property tests,
//! and differential testing utilities.

#![deny(unsafe_code)]

mod fixed_props;
mod spsc_diff;
#[cfg(kani)]
mod spsc_proofs;
mod spsc_props;
