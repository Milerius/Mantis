//! Constant-time types and operations.
//!
//! Prevents the compiler from optimizing bitwise operations into
//! conditional branches, protecting against timing side-channels.
//! Maps from Constantine's `constant_time/` module.

pub(crate) mod ct_routines;
pub mod ct_types;

pub use ct_types::{Borrow, CTBool, Carry, Ct, VarTime};
