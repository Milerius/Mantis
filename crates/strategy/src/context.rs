//! Optional per-strategy runtime bundle.

use mantis_market_state::{MarketStateEngine, OrderBook};

use crate::order_tracker::OrderTracker;
use crate::position::Position;
use crate::queue::QueueEstimator;
use crate::risk::RiskLimits;

/// Optional per-strategy runtime bundle.
///
/// Strategies MAY use this for convenience, or compose their own internals.
/// The `Strategy` trait does NOT require `StrategyContext`.
///
/// Generic over book type — monomorphized, no heap.
pub struct StrategyContext<B: OrderBook, const MAX: usize> {
    /// Book reconstruction engine (strategy's own copy).
    pub engine: MarketStateEngine<B, MAX>,
    /// L2 queue position estimator.
    pub queue: QueueEstimator,
    /// Open order tracking.
    pub orders: OrderTracker,
    /// Per-strategy risk configuration.
    pub risk: RiskLimits,
    /// Per-instrument positions (fixed array).
    pub positions: [Position; MAX],
}
