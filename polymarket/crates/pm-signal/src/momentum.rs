//! Momentum confirmation strategy.
//!
//! Fires mid-window when a directional move that started early has been
//! sustained long enough to confirm trend continuation.

use pm_types::{EntryDecision, MarketState, StrategyId};

use crate::strategy_trait::Strategy;

// ─── MomentumConfirmation ────────────────────────────────────────────────────

/// Enters in the direction of a sustained mid-window move.
///
/// Entry conditions (all must be true):
/// - `min_entry_time_secs <= time_elapsed_secs <= max_entry_time_secs`
/// - `spot_magnitude >= min_spot_magnitude`
/// - `direction_ask <= max_entry_price`
pub struct MomentumConfirmation {
    /// Earliest seconds elapsed before this strategy activates.
    pub min_entry_time_secs: u64,
    /// Latest seconds elapsed after which this strategy no longer fires.
    pub max_entry_time_secs: u64,
    /// Minimum absolute fractional spot move required (e.g. `0.005` = 0.5 %).
    pub min_spot_magnitude: f64,
    /// Maximum contract ask price to enter.
    pub max_entry_price: f64,
}

impl MomentumConfirmation {
    /// Construct a new [`MomentumConfirmation`] strategy.
    #[inline]
    #[must_use]
    pub fn new(
        min_entry_time_secs: u64,
        max_entry_time_secs: u64,
        min_spot_magnitude: f64,
        max_entry_price: f64,
    ) -> Self {
        Self {
            min_entry_time_secs,
            max_entry_time_secs,
            min_spot_magnitude,
            max_entry_price,
        }
    }
}

impl Strategy for MomentumConfirmation {
    fn id(&self) -> StrategyId {
        StrategyId::MomentumConfirmation
    }

    fn evaluate(&self, state: &MarketState) -> Option<EntryDecision> {
        // Scale time boundaries proportionally to timeframe duration.
        // Configured values are for a 15m (900s) window.
        let scale = state.timeframe.duration_secs() as f64 / 900.0;
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss,
                 reason = "scale × time is always a small positive number")]
        let eff_min = (self.min_entry_time_secs as f64 * scale) as u64;
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss,
                 reason = "scale × time is always a small positive number")]
        let eff_max = (self.max_entry_time_secs as f64 * scale) as u64;

        // Must be within the momentum window.
        if state.time_elapsed_secs < eff_min || state.time_elapsed_secs > eff_max {
            return None;
        }

        // Spot must show sustained momentum.
        if state.spot_magnitude < self.min_spot_magnitude {
            return None;
        }

        // Contract must be available and affordable.
        let ask = state.direction_ask()?;
        if ask.as_f64() > self.max_entry_price {
            return None;
        }

        // Confidence: how centred we are in the momentum window ×
        // how much margin we have below max price.
        #[expect(
            clippy::cast_precision_loss,
            reason = "time values are at most hours in seconds; precision loss is negligible"
        )]
        let window_len = self
            .max_entry_time_secs
            .saturating_sub(self.min_entry_time_secs) as f64;
        let position_in_window = if window_len == 0.0 {
            1.0
        } else {
            #[expect(
                clippy::cast_precision_loss,
                reason = "time values are at most hours in seconds; precision loss is negligible"
            )]
            let elapsed_past_min = state
                .time_elapsed_secs
                .saturating_sub(self.min_entry_time_secs) as f64;
            1.0 - (2.0 * elapsed_past_min / window_len - 1.0).abs()
        };
        let price_margin = (self.max_entry_price - ask.as_f64()) / self.max_entry_price;
        let confidence = ((position_in_window + price_margin) * 0.5).clamp(0.0, 1.0);

        Some(EntryDecision {
            side: state.spot_direction,
            limit_price: ask,
            confidence,
            strategy_id: StrategyId::MomentumConfirmation,
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

    fn make_state(elapsed: u64, magnitude: f64, ask_direction: Option<f64>) -> MarketState {
        MarketState {
            asset: Asset::Btc,
            timeframe: Timeframe::Hour1,
            window_id: WindowId::new(2),
            window_open_price: Price::new(100.0).expect("valid"),
            current_spot: Price::new(101.5).expect("valid"),
            spot_magnitude: magnitude,
            spot_direction: Side::Up,
            time_elapsed_secs: elapsed,
            time_remaining_secs: 3600_u64.saturating_sub(elapsed),
            contract_ask_up: ask_direction.and_then(ContractPrice::new),
            contract_ask_down: ContractPrice::new(0.45),
            contract_bid_up: ask_direction.and_then(|v| ContractPrice::new(v - 0.02)),
            contract_bid_down: ContractPrice::new(0.43),
        }
    }

    #[test]
    fn fires_mid_window_with_sustained_momentum() {
        // Window: 300–900s elapsed, magnitude >= 0.005, ask <= 0.65
        let strategy = MomentumConfirmation::new(300, 900, 0.005, 0.65);
        let state = make_state(600, 0.01, Some(0.55));
        let d = strategy.evaluate(&state);
        assert!(d.is_some(), "expected momentum to fire mid-window");
        let d = d.expect("checked above");
        assert_eq!(d.side, Side::Up);
        assert_eq!(d.strategy_id, StrategyId::MomentumConfirmation);
    }

    #[test]
    fn does_not_fire_too_early() {
        let strategy = MomentumConfirmation::new(300, 900, 0.005, 0.65);
        // elapsed=200 < min_entry_time=300
        let state = make_state(200, 0.01, Some(0.55));
        assert!(strategy.evaluate(&state).is_none());
    }

    #[test]
    fn does_not_fire_too_late() {
        let strategy = MomentumConfirmation::new(300, 900, 0.005, 0.65);
        // elapsed=1000 > max_entry_time=900
        let state = make_state(1000, 0.01, Some(0.55));
        assert!(strategy.evaluate(&state).is_none());
    }

    #[test]
    fn does_not_fire_magnitude_too_small() {
        let strategy = MomentumConfirmation::new(300, 900, 0.005, 0.65);
        // magnitude=0.002 < 0.005
        let state = make_state(600, 0.002, Some(0.55));
        assert!(strategy.evaluate(&state).is_none());
    }

    #[test]
    fn does_not_fire_price_too_high() {
        let strategy = MomentumConfirmation::new(300, 900, 0.005, 0.65);
        // ask=0.70 > 0.65
        let state = make_state(600, 0.01, Some(0.70));
        assert!(strategy.evaluate(&state).is_none());
    }

    #[test]
    fn does_not_fire_when_no_contract_data() {
        let strategy = MomentumConfirmation::new(300, 900, 0.005, 0.65);
        let state = make_state(600, 0.01, None);
        assert!(strategy.evaluate(&state).is_none());
    }

    #[test]
    fn id_is_momentum_confirmation() {
        let strategy = MomentumConfirmation::new(300, 900, 0.005, 0.65);
        assert_eq!(strategy.id(), StrategyId::MomentumConfirmation);
    }
}
