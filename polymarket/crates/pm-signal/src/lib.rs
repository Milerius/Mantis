//! Signal engine for Polymarket crypto Up/Down temporal arbitrage.
//!
//! `no_std` by default. Pure math, zero I/O.

#![no_std]
#![deny(unsafe_code)]

#[cfg(feature = "std")]
extern crate std;

pub mod arb;
pub mod early;
pub mod engine;
pub mod estimator;
pub mod hedge;
pub mod logistic;
pub mod lookup;
pub mod momentum;
pub mod multi;
pub mod strategy_trait;

#[cfg(feature = "std")]
pub mod builder;

pub use arb::CompleteSetArb;
pub use early::EarlyDirectional;
pub use engine::SignalEngine;
pub use estimator::FairValueEstimator;
pub use hedge::HedgeLock;
pub use logistic::{Coefficients, LogisticModel};
pub use lookup::{LookupCell, LookupTable, MAG_BUCKETS, TIME_BUCKETS};
pub use momentum::MomentumConfirmation;
pub use multi::{AnyStrategy, Decisions, MAX_STRATEGIES, StrategyEngine};
pub use strategy_trait::Strategy;

#[cfg(feature = "std")]
pub use builder::build_engine_from_config;
