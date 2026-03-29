//! Signal engine for Polymarket crypto Up/Down temporal arbitrage.
//!
//! `no_std` by default. Pure math, zero I/O.

#![no_std]
#![deny(unsafe_code)]

#[cfg(feature = "std")]
extern crate std;

pub mod engine;
pub mod estimator;
pub mod logistic;
pub mod lookup;

pub use engine::SignalEngine;
pub use estimator::FairValueEstimator;
pub use logistic::{Coefficients, LogisticModel};
pub use lookup::{LookupCell, LookupTable, MAG_BUCKETS, TIME_BUCKETS};
