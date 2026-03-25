//! Formal verification and property-based testing for the Mantis SDK.
//!
//! Contains kani proof harnesses, bolero property tests,
//! and differential testing utilities.

#![deny(unsafe_code)]

/// Placeholder — actual proof harnesses added alongside the primitives they verify.
pub fn placeholder() {}

#[cfg(test)]
mod tests {
    #[test]
    fn verify_crate_compiles() {
        super::placeholder();
    }
}
