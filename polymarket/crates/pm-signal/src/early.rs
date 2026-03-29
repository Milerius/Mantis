//! Early directional entry strategy.
//!
//! Fires shortly after window open when the spot price has already moved
//! significantly and the contract is still cheap enough to buy.

use pm_types::{EntryDecision, MarketState, StrategyId};

use crate::strategy_trait::Strategy;

// ─── EarlyDirectional ────────────────────────────────────────────────────────

/// Enters in the direction of the opening move while the window is still young.
///
/// Entry conditions (all must be true):
/// - `time_elapsed_secs <= max_entry_time_secs`
/// - `spot_magnitude >= min_spot_magnitude`
/// - `direction_ask <= max_entry_price`
pub struct EarlyDirectional {
    /// Maximum seconds elapsed since window open to still be considered early.
    pub max_entry_time_secs: u64,
    /// Minimum absolute fractional spot move required (e.g. `0.005` = 0.5 %).
    pub min_spot_magnitude: f64,
    /// Maximum contract ask price to enter (e.g. `0.65`).
    pub max_entry_price: f64,
}

impl EarlyDirectional {
    /// Construct a new [`EarlyDirectional`] strategy.
    #[inline]
    #[must_use]
    pub fn new(max_entry_time_secs: u64, min_spot_magnitude: f64, max_entry_price: f64) -> Self {
        Self {
            max_entry_time_secs,
            min_spot_magnitude,
            max_entry_price,
        }
    }
}

impl Strategy for EarlyDirectional {
    fn id(&self) -> StrategyId {
        StrategyId::EarlyDirectional
    }

    fn evaluate(&self, state: &MarketState) -> Option<EntryDecision> {
        // Must be in the early window.
        if state.time_elapsed_secs > self.max_entry_time_secs {
            return None;
        }

        // Spot must have moved enough.
        if state.spot_magnitude < self.min_spot_magnitude {
            return None;
        }

        // Contract must be cheap enough.
        let ask = state.direction_ask()?;
        if ask.as_f64() > self.max_entry_price {
            return None;
        }

        // Confidence: how early we are (earlier → more time to compound) ×
        // how much margin we have below the max price.
        let time_fraction = if self.max_entry_time_secs == 0 {
            1.0
        } else {
            #[expect(
                clippy::cast_precision_loss,
                reason = "time values are at most a few hours in seconds; precision loss is negligible"
            )]
            let ratio =
                (state.time_elapsed_secs as f64) / (self.max_entry_time_secs as f64);
            1.0 - ratio
        };
        let price_margin = (self.max_entry_price - ask.as_f64()) / self.max_entry_price;
        let confidence = ((time_fraction + price_margin) * 0.5).clamp(0.0, 1.0);

        Some(EntryDecision {
            side: state.spot_direction,
            limit_price: ask,
            confidence,
            strategy_id: StrategyId::EarlyDirectional,
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

    fn make_state(
        direction: Side,
        magnitude: f64,
        elapsed: u64,
        ask_direction: Option<f64>,
    ) -> MarketState {
        let (ask_up, ask_down) = match direction {
            Side::Up => (ask_direction, Some(0.45)),
            Side::Down => (Some(0.45), ask_direction),
        };
        MarketState {
            asset: Asset::Btc,
            timeframe: Timeframe::Hour1,
            window_id: WindowId::new(1),
            window_open_price: Price::new(100.0).expect("valid"),
            current_spot: Price::new(101.0).expect("valid"),
            spot_magnitude: magnitude,
            spot_direction: direction,
            time_elapsed_secs: elapsed,
            time_remaining_secs: 3600_u64.saturating_sub(elapsed),
            contract_ask_up: ask_up.and_then(ContractPrice::new),
            contract_ask_down: ask_down.and_then(ContractPrice::new),
            contract_bid_up: ask_up.and_then(|v| ContractPrice::new(v - 0.02)),
            contract_bid_down: ask_down.and_then(|v| ContractPrice::new(v - 0.02)),
        }
    }

    #[test]
    fn fires_early_with_sufficient_magnitude_and_cheap_price() {
        let strategy = EarlyDirectional::new(300, 0.005, 0.65);
        // elapsed=120 <= 300, magnitude=0.01 >= 0.005, ask=0.55 <= 0.65
        let state = make_state(Side::Up, 0.01, 120, Some(0.55));
        let d = strategy.evaluate(&state);
        assert!(d.is_some(), "expected early directional to fire");
        let d = d.expect("checked above");
        assert_eq!(d.side, Side::Up);
        assert_eq!(d.strategy_id, StrategyId::EarlyDirectional);
    }

    #[test]
    fn does_not_fire_when_too_late() {
        let strategy = EarlyDirectional::new(300, 0.005, 0.65);
        // elapsed=400 > 300
        let state = make_state(Side::Up, 0.01, 400, Some(0.55));
        assert!(strategy.evaluate(&state).is_none());
    }

    #[test]
    fn does_not_fire_when_magnitude_too_small() {
        let strategy = EarlyDirectional::new(300, 0.005, 0.65);
        // magnitude=0.002 < 0.005
        let state = make_state(Side::Up, 0.002, 100, Some(0.55));
        assert!(strategy.evaluate(&state).is_none());
    }

    #[test]
    fn does_not_fire_when_price_too_high() {
        let strategy = EarlyDirectional::new(300, 0.005, 0.65);
        // ask=0.70 > 0.65
        let state = make_state(Side::Up, 0.01, 100, Some(0.70));
        assert!(strategy.evaluate(&state).is_none());
    }

    #[test]
    fn does_not_fire_when_no_contract_data() {
        let strategy = EarlyDirectional::new(300, 0.005, 0.65);
        let state = make_state(Side::Up, 0.01, 100, None);
        assert!(strategy.evaluate(&state).is_none());
    }

    #[test]
    fn id_is_early_directional() {
        let strategy = EarlyDirectional::new(300, 0.005, 0.65);
        assert_eq!(strategy.id(), StrategyId::EarlyDirectional);
    }
}
