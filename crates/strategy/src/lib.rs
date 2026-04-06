//! Venue-agnostic strategy runtime primitives for the Mantis SDK.
//!
//! Provides the `Strategy` trait, `OrderIntent`, position tracking,
//! queue estimation, and risk limits. All types are `no_std` compatible,
//! use fixed-size arrays, and allocate nothing on the heap.
//!
//! Prediction-market-specific types (YES/NO positions, settlement `PnL`,
//! merge operations) belong in `mantis-prediction`, not here.

#![no_std]
#![deny(unsafe_code)]

pub mod context;
mod exposure;
mod intent;
mod order_tracker;
mod position;
mod queue;
mod risk;
mod traits;

pub use context::StrategyContext;
pub use exposure::ExposureView;
pub use intent::{MAX_INTENTS_PER_TICK, OrderAction, OrderIntent};
pub use order_tracker::{
    MAX_TRACKED_ORDERS, OrderState, OrderTracker, TrackedOrder, TrackerFullError,
};
pub use position::Position;
pub use queue::{MAX_QUEUED_ORDERS, QueueEstimator, QueuedOrder};
pub use risk::{RiskCheckResult, RiskLimits};
pub use traits::Strategy;
