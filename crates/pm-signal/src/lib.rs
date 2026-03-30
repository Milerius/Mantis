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
pub mod entry_timer;
pub mod estimator;
pub mod hedge;
pub mod logistic;
pub mod lookup;
pub mod momentum;
pub mod multi;
pub mod strategy_trait;
pub mod trend_filter;

#[cfg(feature = "std")]
pub mod builder;
#[cfg(feature = "std")]
pub mod instance;

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
pub use entry_timer::{EntryTimer, PendingEntry};
pub use trend_filter::TrendFilter;

#[cfg(feature = "std")]
pub use builder::{build_engine_from_config, build_instances_from_config};
#[cfg(feature = "std")]
pub use instance::ConcreteStrategyInstance;
