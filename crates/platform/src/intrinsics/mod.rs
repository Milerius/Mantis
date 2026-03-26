//! Platform intrinsics: extended precision, carry/borrow arithmetic,
//! compiler hints, copy policies.

pub mod addcarry_subborrow;
pub mod compiler_hints;
pub mod extended_prec;

pub use addcarry_subborrow::{AddCarryOp, SubBorrowOp};
pub use compiler_hints::{PrefetchLocality, PrefetchRW, prefetch, prefetch_large};
pub use extended_prec::{
    SignedWideMul, WideMul, WideMulAdd1, WideMulAdd2,
    mul_acc, mul_acc32, mul_double_acc, mul_double_acc32,
};
