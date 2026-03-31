//! Complete-set arbitrage strategy.
//!
//! Fires when the combined cost of buying both the Up and Down contracts is
//! below `max_combined_cost` and the implied profit-per-share exceeds
//! `min_profit_per_share`.

use pm_types::{EntryDecision, MarketState, Side, StrategyId, StrategyLabel};

use crate::strategy_trait::Strategy;

// ─── CompleteSetArb ──────────────────────────────────────────────────────────

/// Exploits mis-pricing where Up + Down ask prices sum to less than $1.
///
/// On a Polymarket binary market the two outcomes must settle to exactly $1
/// combined.  When the market allows you to buy both for less than $1, the
/// position is riskless at expiry.
pub struct CompleteSetArb {
    /// Maximum acceptable combined ask (Up + Down) to trigger entry.
    ///
    /// For example `0.98` means the combined ask must be below 98 cents.
    pub max_combined_cost: f64,
    /// Minimum profit per share required to trigger entry (i.e. `1.0 - combined`).
    pub min_profit_per_share: f64,
}

impl CompleteSetArb {
    /// Construct a new [`CompleteSetArb`] strategy.
    #[inline]
    #[must_use]
    pub fn new(max_combined_cost: f64, min_profit_per_share: f64) -> Self {
        Self {
            max_combined_cost,
            min_profit_per_share,
        }
    }
}

impl Strategy for CompleteSetArb {
    fn id(&self) -> StrategyId {
        StrategyId::CompleteSetArb
    }

    fn evaluate(&self, state: &MarketState) -> Option<EntryDecision> {
        let ask_up = state.contract_ask_up?.as_f64();
        let ask_down = state.contract_ask_down?.as_f64();

        let combined = ask_up + ask_down;
        let profit = 1.0 - combined;

        if combined >= self.max_combined_cost || profit < self.min_profit_per_share {
            return None;
        }

        // Buy whichever side is cheaper (lower ask → more shares per dollar).
        let (side, limit_price) = if ask_up <= ask_down {
            (Side::Up, state.contract_ask_up?)
        } else {
            (Side::Down, state.contract_ask_down?)
        };

        // Confidence scales linearly with profit margin (capped at 1.0).
        let confidence = (profit / (1.0 - self.max_combined_cost)).min(1.0);

        Some(EntryDecision {
            side,
            limit_price,
            confidence,
            strategy_id: StrategyId::CompleteSetArb,
            label: StrategyLabel::EMPTY,
        })
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
            window_id: WindowId::new(1),
            window_open_price: Price::new(100.0).expect("valid"),
            current_spot: Price::new(101.0).expect("valid"),
            spot_magnitude: 0.01,
            spot_direction: Side::Up,
            time_elapsed_secs: 600,
            time_remaining_secs: 3000,
            contract_ask_up: ask_up.and_then(ContractPrice::new),
            contract_ask_down: ask_down.and_then(ContractPrice::new),
            contract_bid_up: ask_up.and_then(|v| ContractPrice::new(v - 0.02)),
            contract_bid_down: ask_down.and_then(|v| ContractPrice::new(v - 0.02)),
            orderbook_imbalance: None,
        }
    }

    #[test]
    fn arb_fires_when_combined_below_threshold() {
        let strategy = CompleteSetArb::new(0.98, 0.015);
        // combined = 0.48 + 0.49 = 0.97 < 0.98; profit = 0.03 > 0.015
        let state = make_state(Some(0.48), Some(0.49));
        let decision = strategy.evaluate(&state);
        assert!(decision.is_some(), "expected arb to fire");
        let d = decision.expect("checked above");
        assert_eq!(d.strategy_id, StrategyId::CompleteSetArb);
        assert!(d.confidence > 0.0);
    }

    #[test]
    fn arb_does_not_fire_when_combined_too_high() {
        let strategy = CompleteSetArb::new(0.98, 0.015);
        // combined = 0.50 + 0.50 = 1.00 >= 0.98
        let state = make_state(Some(0.50), Some(0.50));
        assert!(strategy.evaluate(&state).is_none());
    }

    #[test]
    fn arb_does_not_fire_when_profit_too_small() {
        let strategy = CompleteSetArb::new(0.98, 0.025);
        // combined = 0.48 + 0.49 = 0.97; profit = 0.03 > 0.025, should fire
        // change thresholds so profit is too small
        let strategy2 = CompleteSetArb::new(0.98, 0.05);
        // combined = 0.48 + 0.49 = 0.97; profit = 0.03 < 0.05 → no fire
        let state = make_state(Some(0.48), Some(0.49));
        assert!(strategy2.evaluate(&state).is_none());
        // Confirm strategy fires when threshold is met
        let decision = strategy.evaluate(&state);
        assert!(decision.is_some());
    }

    #[test]
    fn arb_does_not_fire_when_ask_up_missing() {
        let strategy = CompleteSetArb::new(0.98, 0.015);
        let state = make_state(None, Some(0.49));
        assert!(strategy.evaluate(&state).is_none());
    }

    #[test]
    fn arb_does_not_fire_when_ask_down_missing() {
        let strategy = CompleteSetArb::new(0.98, 0.015);
        let state = make_state(Some(0.48), None);
        assert!(strategy.evaluate(&state).is_none());
    }

    #[test]
    fn arb_buys_cheaper_side() {
        let strategy = CompleteSetArb::new(0.98, 0.015);
        // Down ask is lower → should buy Down
        let state = make_state(Some(0.50), Some(0.46));
        let decision = strategy.evaluate(&state).expect("arb should fire");
        assert_eq!(decision.side, Side::Down);
    }

    #[test]
    fn arb_id_is_complete_set_arb() {
        let strategy = CompleteSetArb::new(0.98, 0.015);
        assert_eq!(strategy.id(), StrategyId::CompleteSetArb);
    }
}
