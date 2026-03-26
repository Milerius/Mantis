//! Constant-time types and operations.
//!
//! Prevents the compiler from optimizing bitwise operations into
//! conditional branches, protecting against timing side-channels.
//! Maps from Constantine's `constant_time/` module.

pub mod ct_division;
pub(crate) mod ct_routines;
pub mod ct_types;
pub mod multiplexers;

pub use ct_division::{div2n1n, div2n1n_u32};
pub use ct_types::{Borrow, CTBool, Carry, Ct, VarTime};
pub use multiplexers::{
    ccopy, ccopy_usize, ccopy32, mux, mux_bool, mux_bool_usize, mux_bool32, mux_usize, mux32,
    secret_lookup,
};
