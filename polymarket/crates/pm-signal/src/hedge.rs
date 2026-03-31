//! Hedge lock strategy.
//!
//! When an existing position is losing, this strategy considers buying the
//! opposite side if the combined cost of both legs is still below $1.

use pm_types::{ContractPrice, EntryDecision, MarketState, Side, StrategyId, StrategyLabel};

use crate::strategy_trait::Strategy;

// ─── HedgeLock ───────────────────────────────────────────────────────────────

/// Caps a losing position by buying the opposite contract.
///
/// The trade only makes sense when the combined cost of the existing entry
/// price plus the hedge ask is still below `max_combined_cost` (i.e., less
/// than $1), ensuring a riskless floor at expiry.
pub struct HedgeLock {
    /// Maximum combined cost (existing entry + hedge ask) to still enter.
    ///
    /// For example `0.98` means the two legs together must cost less than
    /// 98 cents to guarantee a minimum net recovery at expiry.
    pub max_combined_cost: f64,
}

impl HedgeLock {
    /// Construct a new [`HedgeLock`] strategy.
    #[inline]
    #[must_use]
    pub fn new(max_combined_cost: f64) -> Self {
        Self { max_combined_cost }
    }

    /// Full evaluation with explicit position context.
    ///
    /// `existing_entry` is the average entry price of the current losing
    /// position, and `existing_side` is its direction.  Returns `Some` only
    /// when buying the opposite side is still profitable at expiry.
    #[must_use]
    pub fn evaluate_with_position(
        &self,
        state: &MarketState,
        existing_entry: ContractPrice,
        existing_side: Side,
    ) -> Option<EntryDecision> {
        // The hedge side is opposite to the existing position.
        let hedge_side = existing_side.opposite();

        let hedge_ask = match hedge_side {
            Side::Up => state.contract_ask_up?,
            Side::Down => state.contract_ask_down?,
        };

        let combined = existing_entry.as_f64() + hedge_ask.as_f64();
        if combined >= self.max_combined_cost {
            return None;
        }

        // Confidence: how much room we have below the threshold (linear scale).
        let slack = self.max_combined_cost - combined;
        let confidence = (slack / (1.0 - self.max_combined_cost)).clamp(0.0, 1.0);

        Some(EntryDecision {
            side: hedge_side,
            limit_price: hedge_ask,
            confidence,
            strategy_id: StrategyId::HedgeLock,
            label: StrategyLabel::EMPTY,
        })
    }
}

/// The base [`Strategy`] trait implementation returns `None` because
/// [`HedgeLock`] requires position context that is not present in
/// [`MarketState`].  Callers should use [`HedgeLock::evaluate_with_position`]
/// directly.
impl Strategy for HedgeLock {
    fn id(&self) -> StrategyId {
        StrategyId::HedgeLock
    }

    fn evaluate(&self, _state: &MarketState) -> Option<EntryDecision> {
        // Position context required — use evaluate_with_position instead.
        None
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    extern crate std;

    use pm_types::{Asset, ContractPrice, Price, Side, Timeframe, WindowId};

    use super::*;
    use crate::strategy_trait::Strategy;

    fn make_state(ask_up: Option<f64>, ask_down: Option<f64>) -> MarketState {
        MarketState {
            asset: Asset::Btc,
            timeframe: Timeframe::Hour1,
            window_id: WindowId::new(3),
            window_open_price: Price::new(100.0).expect("valid"),
            current_spot: Price::new(99.0).expect("valid"),
            spot_magnitude: 0.01,
            spot_direction: Side::Down,
            time_elapsed_secs: 1800,
            time_remaining_secs: 1800,
            contract_ask_up: ask_up.and_then(ContractPrice::new),
            contract_ask_down: ask_down.and_then(ContractPrice::new),
            contract_bid_up: ask_up.and_then(|v| ContractPrice::new(v - 0.02)),
            contract_bid_down: ask_down.and_then(|v| ContractPrice::new(v - 0.02)),
            orderbook_imbalance: None,
            binance_price: None,
            okx_price: None,
            momentum_score: 0.0,
        }
    }

    /// Existing position: bought Up at 0.55, price moved Down → losing.
    fn existing() -> (ContractPrice, Side) {
        (ContractPrice::new(0.55).expect("valid"), Side::Up)
    }

    #[test]
    fn hedge_fires_when_combined_below_threshold() {
        let strategy = HedgeLock::new(0.98);
        // existing entry = 0.55, Down ask = 0.40 → combined = 0.95 < 0.98
        let state = make_state(Some(0.55), Some(0.40));
        let (entry, side) = existing();
        let d = strategy.evaluate_with_position(&state, entry, side);
        assert!(d.is_some(), "expected hedge to fire");
        let d = d.expect("checked above");
        assert_eq!(d.side, Side::Down); // opposite of Up
        assert_eq!(d.strategy_id, StrategyId::HedgeLock);
        assert!(d.confidence > 0.0);
    }

    #[test]
    fn hedge_does_not_fire_when_combined_too_high() {
        let strategy = HedgeLock::new(0.98);
        // existing entry = 0.55, Down ask = 0.50 → combined = 1.05 > 0.98
        let state = make_state(Some(0.55), Some(0.50));
        let (entry, side) = existing();
        assert!(
            strategy
                .evaluate_with_position(&state, entry, side)
                .is_none()
        );
    }

    #[test]
    fn hedge_does_not_fire_when_ask_missing() {
        let strategy = HedgeLock::new(0.98);
        // Down ask is None
        let state = make_state(Some(0.55), None);
        let (entry, side) = existing();
        assert!(
            strategy
                .evaluate_with_position(&state, entry, side)
                .is_none()
        );
    }

    #[test]
    fn trait_evaluate_always_returns_none() {
        let strategy = HedgeLock::new(0.98);
        let state = make_state(Some(0.45), Some(0.40));
        // Base trait evaluate requires no position context → always None
        assert!(strategy.evaluate(&state).is_none());
    }

    #[test]
    fn id_is_hedge_lock() {
        let strategy = HedgeLock::new(0.98);
        assert_eq!(strategy.id(), StrategyId::HedgeLock);
    }
}
