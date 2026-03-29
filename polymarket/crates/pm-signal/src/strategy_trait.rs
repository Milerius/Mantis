//! Core [`Strategy`] trait consumed by the [`StrategyEngine`].

use pm_types::{EntryDecision, MarketState, StrategyId};

// ─── Strategy ────────────────────────────────────────────────────────────────

/// A pluggable entry strategy for the multi-strategy engine.
///
/// Each implementation inspects a [`MarketState`] snapshot and either returns
/// an [`EntryDecision`] or `None` if its entry conditions are not met.
pub trait Strategy: Send + Sync {
    /// Unique identifier for this strategy.
    fn id(&self) -> StrategyId;

    /// Evaluate the current market state and return an entry decision, or
    /// `None` if conditions are not satisfied.
    fn evaluate(&self, state: &MarketState) -> Option<EntryDecision>;
}
