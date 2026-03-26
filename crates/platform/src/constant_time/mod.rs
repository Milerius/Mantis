//! Constant-time types and operations.
//!
//! Prevents the compiler from optimizing bitwise operations into
//! conditional branches, protecting against timing side-channels.
//! Maps from Constantine's `constant_time/` module.

pub(crate) mod ct_routines;
pub mod ct_types;
pub mod multiplexers;

pub use ct_types::{Borrow, CTBool, Carry, Ct, VarTime};
pub use multiplexers::{ccopy, ccopy32, ccopy_usize, mux, mux32, mux_bool, mux_bool32,
    mux_bool_usize, mux_usize, secret_lookup};
