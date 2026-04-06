//! Venue-agnostic strategy runtime primitives for the Mantis SDK.
//!
//! Provides the `Strategy` trait, `OrderIntent`, position tracking,
//! queue estimation, and risk limits. All types are `no_std` compatible,
//! use fixed-size arrays, and allocate nothing on the heap.
//!
//! Prediction-market-specific types (YES/NO positions, settlement PnL,
//! merge operations) belong in `mantis-prediction`, not here.

#![no_std]
#![deny(unsafe_code)]

mod intent;
mod position;
mod traits;

pub use intent::{MAX_INTENTS_PER_TICK, OrderAction, OrderIntent};
pub use position::Position;
pub use traits::Strategy;
