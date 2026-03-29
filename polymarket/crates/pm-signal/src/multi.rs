//! Multi-strategy engine: runs all registered strategies and picks the best.
//!
//! Requires heap allocation — available when the `std` feature is enabled.

extern crate alloc;

use alloc::{boxed::Box, vec::Vec};
use pm_types::{EntryDecision, MarketState};

use crate::strategy_trait::Strategy;

// ─── StrategyEngine ──────────────────────────────────────────────────────────

/// Runs multiple strategies against a [`MarketState`] and selects the best.
///
/// Strategies are evaluated in registration order.  [`StrategyEngine::evaluate`]
/// returns the highest-confidence decision; [`StrategyEngine::evaluate_all`]
/// returns every decision that fired (useful for per-strategy logging).
pub struct StrategyEngine {
    strategies: Vec<Box<dyn Strategy>>,
}

impl StrategyEngine {
    /// Construct a new engine from a list of boxed strategies.
    #[inline]
    #[must_use]
    pub fn new(strategies: Vec<Box<dyn Strategy>>) -> Self {
        Self { strategies }
    }

    /// Evaluate all strategies and return the one with the highest confidence.
    ///
    /// Returns `None` if no strategy fires.
    #[must_use]
    pub fn evaluate(&self, state: &MarketState) -> Option<EntryDecision> {
        self.strategies
            .iter()
            .filter_map(|s| s.evaluate(state))
            .reduce(|best, next| {
                if next.confidence > best.confidence {
                    next
                } else {
                    best
                }
            })
    }

    /// Evaluate all strategies and return every decision that fired.
    ///
    /// The returned vec is in the same order as the registered strategies.
    /// Returns an empty vec if no strategy fires.
    #[must_use]
    pub fn evaluate_all(&self, state: &MarketState) -> Vec<EntryDecision> {
        self.strategies
            .iter()
            .filter_map(|s| s.evaluate(state))
            .collect()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    extern crate std;
    use alloc::{boxed::Box, vec};

    use pm_types::{Asset, ContractPrice, EntryDecision, MarketState, Price, Side, StrategyId, Timeframe, WindowId};

    use super::*;
    use crate::strategy_trait::Strategy;

    // ── Stub strategies ──────────────────────────────────────────────────────

    struct AlwaysFires {
        confidence: f64,
        id: StrategyId,
    }

    impl Strategy for AlwaysFires {
        fn id(&self) -> StrategyId {
            self.id
        }
        fn evaluate(&self, state: &MarketState) -> Option<EntryDecision> {
            Some(EntryDecision {
                side: state.spot_direction,
                limit_price: ContractPrice::new(0.50).expect("valid"),
                confidence: self.confidence,
                strategy_id: self.id,
            })
        }
    }

    struct NeverFires;
    impl Strategy for NeverFires {
        fn id(&self) -> StrategyId {
            StrategyId::HedgeLock
        }
        fn evaluate(&self, _state: &MarketState) -> Option<EntryDecision> {
            None
        }
    }

    fn make_state() -> MarketState {
        MarketState {
            asset: Asset::Btc,
            timeframe: Timeframe::Hour1,
            window_id: WindowId::new(1),
            window_open_price: Price::new(100.0).expect("valid"),
            current_spot: Price::new(102.0).expect("valid"),
            spot_magnitude: 0.02,
            spot_direction: Side::Up,
            time_elapsed_secs: 600,
            time_remaining_secs: 3000,
            contract_ask_up: ContractPrice::new(0.55),
            contract_ask_down: ContractPrice::new(0.48),
            contract_bid_up: ContractPrice::new(0.53),
            contract_bid_down: ContractPrice::new(0.46),
        }
    }

    // ── Tests ────────────────────────────────────────────────────────────────

    #[test]
    fn highest_confidence_wins_when_multiple_fire() {
        let strategies: Vec<Box<dyn Strategy>> = vec![
            Box::new(AlwaysFires {
                confidence: 0.4,
                id: StrategyId::EarlyDirectional,
            }),
            Box::new(AlwaysFires {
                confidence: 0.8,
                id: StrategyId::MomentumConfirmation,
            }),
            Box::new(AlwaysFires {
                confidence: 0.6,
                id: StrategyId::CompleteSetArb,
            }),
        ];
        let engine = StrategyEngine::new(strategies);
        let decision = engine.evaluate(&make_state());
        assert!(decision.is_some());
        let d = decision.expect("checked above");
        assert_eq!(d.strategy_id, StrategyId::MomentumConfirmation);
        assert!((d.confidence - 0.8).abs() < 1e-10);
    }

    #[test]
    fn single_strategy_returns_its_decision() {
        let strategies: Vec<Box<dyn Strategy>> = vec![Box::new(AlwaysFires {
            confidence: 0.7,
            id: StrategyId::EarlyDirectional,
        })];
        let engine = StrategyEngine::new(strategies);
        let decision = engine.evaluate(&make_state());
        assert!(decision.is_some());
        assert_eq!(
            decision.expect("should have decision").strategy_id,
            StrategyId::EarlyDirectional
        );
    }

    #[test]
    fn no_strategies_fire_returns_none() {
        let strategies: Vec<Box<dyn Strategy>> =
            vec![Box::new(NeverFires), Box::new(NeverFires)];
        let engine = StrategyEngine::new(strategies);
        assert!(engine.evaluate(&make_state()).is_none());
    }

    #[test]
    fn empty_engine_returns_none() {
        let engine = StrategyEngine::new(vec![]);
        assert!(engine.evaluate(&make_state()).is_none());
    }

    #[test]
    fn evaluate_all_returns_all_decisions() {
        let strategies: Vec<Box<dyn Strategy>> = vec![
            Box::new(AlwaysFires {
                confidence: 0.4,
                id: StrategyId::EarlyDirectional,
            }),
            Box::new(NeverFires),
            Box::new(AlwaysFires {
                confidence: 0.7,
                id: StrategyId::CompleteSetArb,
            }),
        ];
        let engine = StrategyEngine::new(strategies);
        let all = engine.evaluate_all(&make_state());
        assert_eq!(all.len(), 2, "only 2 strategies fire");
        assert_eq!(all[0].strategy_id, StrategyId::EarlyDirectional);
        assert_eq!(all[1].strategy_id, StrategyId::CompleteSetArb);
    }
}
