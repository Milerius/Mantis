//! Multi-strategy engine: runs all registered strategies and picks the best.
//!
//! Zero-heap-allocation hot path — `evaluate_all` returns a fixed-size
//! [`Decisions`] array rather than a `Vec`.  Enum dispatch (`AnyStrategy`)
//! eliminates vtable indirection on every tick.
//!
//! The `Strategy` trait is retained for extensibility, but `StrategyEngine`
//! uses concrete enum dispatch internally.

extern crate alloc;

use alloc::{boxed::Box, vec::Vec};
use pm_types::{EntryDecision, MarketState};

use crate::{
    CompleteSetArb, EarlyDirectional, HedgeLock, MomentumConfirmation,
    strategy_trait::Strategy,
};

// ─── DecisionsIter type alias ─────────────────────────────────────────────────

/// Iterator type returned by [`Decisions::iter`].
pub type DecisionsIter<'a> = core::iter::FilterMap<
    core::slice::Iter<'a, Option<EntryDecision>>,
    fn(&'a Option<EntryDecision>) -> Option<&'a EntryDecision>,
>;

// ─── MAX_STRATEGIES ──────────────────────────────────────────────────────────

/// Maximum number of strategies that can fire simultaneously.
///
/// Sized for the four concrete strategies in the current engine.  Raise this
/// constant if more strategies are added.
pub const MAX_STRATEGIES: usize = 4;

// ─── Decisions ───────────────────────────────────────────────────────────────

/// Fixed-capacity array of [`EntryDecision`]s.
///
/// Returned by [`StrategyEngine::evaluate_all`] — zero heap allocation on the
/// hot path.
#[derive(Debug, Clone, Copy)]
pub struct Decisions {
    items: [Option<EntryDecision>; MAX_STRATEGIES],
    count: usize,
}

impl Decisions {
    /// Construct an empty [`Decisions`].
    #[inline]
    #[must_use]
    fn new() -> Self {
        Self {
            items: [None; MAX_STRATEGIES],
            count: 0,
        }
    }

    /// Push a decision.  Silently drops it when the array is full (should
    /// never happen with `MAX_STRATEGIES == 4` and four concrete strategies).
    #[inline]
    fn push(&mut self, d: EntryDecision) {
        debug_assert!(
            self.count < MAX_STRATEGIES,
            "Decisions buffer full — increase MAX_STRATEGIES (current: {MAX_STRATEGIES})"
        );
        if self.count < MAX_STRATEGIES {
            self.items[self.count] = Some(d);
            self.count += 1;
        }
    }

    /// Iterate over the decisions that fired.
    #[inline]
    pub fn iter(&self) -> DecisionsIter<'_> {
        self.items[..self.count]
            .iter()
            .filter_map(Option::as_ref)
    }

    /// Number of decisions that fired.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.count
    }

    /// `true` when no strategy fired.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

impl<'a> IntoIterator for &'a Decisions {
    type Item = &'a EntryDecision;
    type IntoIter = DecisionsIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

// ─── AnyStrategy ─────────────────────────────────────────────────────────────

/// Concrete enum that wraps every supported strategy type.
///
/// Avoids heap allocation and vtable indirection.  The `Strategy` trait is
/// still available via `Box<dyn Strategy>` for extension points outside the
/// hot path — `AnyStrategy::Boxed` captures that case.
pub enum AnyStrategy {
    /// [`CompleteSetArb`] variant.
    Arb(CompleteSetArb),
    /// [`EarlyDirectional`] variant.
    Early(EarlyDirectional),
    /// [`MomentumConfirmation`] variant.
    Momentum(MomentumConfirmation),
    /// [`HedgeLock`] variant.
    Hedge(HedgeLock),
    /// Escape hatch: any boxed [`Strategy`] trait object.
    Boxed(Box<dyn Strategy>),
}

impl AnyStrategy {
    /// Evaluate the strategy — no virtual dispatch for the four concrete arms.
    #[inline]
    pub fn evaluate(&self, state: &MarketState) -> Option<EntryDecision> {
        match self {
            Self::Arb(s) => s.evaluate(state),
            Self::Early(s) => s.evaluate(state),
            Self::Momentum(s) => s.evaluate(state),
            Self::Hedge(s) => s.evaluate(state),
            Self::Boxed(s) => s.evaluate(state),
        }
    }
}

// ─── StrategyEngine ──────────────────────────────────────────────────────────

/// Runs multiple strategies against a [`MarketState`] and selects the best.
///
/// Accepts either a `Vec<AnyStrategy>` (zero-alloc evaluate path) **or** the
/// legacy `Vec<Box<dyn Strategy>>` via [`StrategyEngine::new`] for backwards
/// compatibility with existing tests and callers.
///
/// On the hot path (`evaluate_all`) strategies are dispatched through the
/// `AnyStrategy` enum — no vtable, no heap allocation.
pub struct StrategyEngine {
    strategies: Vec<AnyStrategy>,
}

impl StrategyEngine {
    /// Construct a new engine from a list of boxed trait objects.
    ///
    /// Each `Box<dyn Strategy>` is stored as [`AnyStrategy::Boxed`].  This
    /// path is kept for API compatibility; prefer [`StrategyEngine::from_any`]
    /// in performance-sensitive callsites.
    #[inline]
    #[must_use]
    pub fn new(strategies: Vec<Box<dyn Strategy>>) -> Self {
        Self {
            strategies: strategies.into_iter().map(AnyStrategy::Boxed).collect(),
        }
    }

    /// Construct a new engine from a list of concrete [`AnyStrategy`] values.
    ///
    /// This is the zero-vtable path: dispatch goes through the `match` in
    /// [`AnyStrategy::evaluate`] rather than through a fat pointer.
    #[inline]
    #[must_use]
    pub fn from_any(strategies: Vec<AnyStrategy>) -> Self {
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
    /// Returns a zero-allocation [`Decisions`] array.  Order matches the
    /// registration order of the strategies.
    #[must_use]
    pub fn evaluate_all(&self, state: &MarketState) -> Decisions {
        let mut out = Decisions::new();
        for s in &self.strategies {
            if let Some(d) = s.evaluate(state) {
                out.push(d);
            }
        }
        out
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    extern crate std;
    use alloc::{boxed::Box, vec};

    use pm_types::{
        Asset, ContractPrice, EntryDecision, MarketState, Price, Side, StrategyId, StrategyLabel,
        Timeframe, WindowId,
    };

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
                label: StrategyLabel::EMPTY,
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
            orderbook_imbalance: None,
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
        let strategies: Vec<Box<dyn Strategy>> = vec![Box::new(NeverFires), Box::new(NeverFires)];
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
        let decisions: Vec<_> = all.iter().collect();
        assert_eq!(decisions[0].strategy_id, StrategyId::EarlyDirectional);
        assert_eq!(decisions[1].strategy_id, StrategyId::CompleteSetArb);
    }

    // ── AnyStrategy / from_any path ──────────────────────────────────────────

    #[test]
    fn from_any_enum_dispatch_fires_correctly() {
        let engine = StrategyEngine::from_any(vec![
            AnyStrategy::Early(EarlyDirectional::new(300, 0.005, 0.65)),
            AnyStrategy::Momentum(MomentumConfirmation::new(300, 900, 0.005, 0.65)),
            AnyStrategy::Arb(CompleteSetArb::new(0.98, 0.015)),
            AnyStrategy::Hedge(HedgeLock::new(0.98)),
        ]);
        // State: magnitude 0.02, elapsed 600 (within momentum window 300–900),
        // ask_up = 0.55 (≤ 0.65), combined = 0.55 + 0.48 = 1.03 (arb won't fire).
        let all = engine.evaluate_all(&make_state());
        // EarlyDirectional fires (elapsed=600 > max=300 → no; actually 600 > 300 so Early won't fire)
        // MomentumConfirmation fires (300 ≤ 600 ≤ 900, mag 0.02 ≥ 0.005, ask 0.55 ≤ 0.65)
        assert!(
            !all.is_empty(),
            "at least MomentumConfirmation should fire"
        );
    }

    #[test]
    fn decisions_is_empty_when_none_fire() {
        let engine = StrategyEngine::new(vec![]);
        let all = engine.evaluate_all(&make_state());
        assert!(all.is_empty());
        assert_eq!(all.len(), 0);
    }
}
