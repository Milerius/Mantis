//! Compile-time-scaled fixed-point decimal arithmetic.
//!
//! This crate provides [`FixedI64`], a fixed-point decimal type backed by `i64`
//! with a compile-time scale factor of `10^D`.
//!
//! # Architecture
//!
//! `FixedI64<D>` is the numeric engine. Domain-specific semantic types
//! (e.g., `UsdcAmount`, `Probability`) live in `mantis-types` and wrap this type.
//! Hot-path code uses separate tick/lot integer types, not fixed-point.
//!
//! # Validated scales
//!
//! D=2, 4, 6, 8 are tested, benchmarked, and documented as production-ready.
//! Other values up to D=18 compile and work but are not part of the validated set.

#![no_std]
#![deny(unsafe_code)]

#[cfg(feature = "std")]
extern crate std;

mod arithmetic;
mod convert;
mod fixed_i64;
mod fmt;
mod mul_div;
mod parse;

pub use fixed_i64::FixedI64;
pub use parse::ParseFixedError;
