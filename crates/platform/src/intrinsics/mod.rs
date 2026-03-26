//! Platform intrinsics: extended precision, carry/borrow arithmetic,
//! compiler hints, copy policies.

pub mod addcarry_subborrow;

pub use addcarry_subborrow::{AddCarryOp, SubBorrowOp};
