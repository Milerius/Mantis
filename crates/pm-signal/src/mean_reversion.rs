//! Mean reversion strategy.
//!
//! Fades overshoots — when the market has moved too far in one direction
//! late in the window, bet on reversal by entering the opposite side.

use pm_types::{EntryDecision, MarketState, StrategyId, StrategyLabel};

use crate::strategy_trait::Strategy;

// ─── MeanReversion ──────────────────────────────────────────────────────────

/// Enters the opposite side of a suspected overshoot.
///
/// Entry conditions (all must be true):
/// - `time_elapsed_secs >= eff_min_elapsed`
/// - `spot_magnitude >= min_spot_magnitude` (the move we're fading)
/// - `opposite_ask <= max_opposite_price`
pub struct MeanReversion {
    /// Minimum seconds elapsed before this activates (e.g. 180 = after 3 min).
    pub min_elapsed_secs: u64,
    /// Minimum magnitude for the move to be considered "overshot" (e.g. 0.005 = 0.5%).
    pub min_spot_magnitude: f64,
    /// Maximum price for the OPPOSITE side (i.e. how cheap the contrarian bet is).
    pub max_opposite_price: f64,
    /// Human-readable label to distinguish variants.
    pub label: StrategyLabel,
}

impl MeanReversion {
    /// Construct a new [`MeanReversion`] strategy.
    #[inline]
    #[must_use]
    pub fn new(min_elapsed_secs: u64, min_spot_magnitude: f64, max_opposite_price: f64) -> Self {
        Self {
            min_elapsed_secs,
            min_spot_magnitude,
            max_opposite_price,
            label: StrategyLabel::EMPTY,
        }
    }

    /// Construct with an explicit label.
    #[inline]
    #[must_use]
    pub fn with_label(mut self, label: StrategyLabel) -> Self {
        self.label = label;
        self
    }
}

impl Strategy for MeanReversion {
    fn id(&self) -> StrategyId {
        StrategyId::MeanReversion
    }

    fn evaluate(&self, state: &MarketState) -> Option<EntryDecision> {
        // Scale time boundaries proportionally to timeframe duration.
        // Configured values are for a 5m (300s) window.
        #[expect(clippy::cast_precision_loss,
                 reason = "duration_secs() is at most a few hours in seconds; precision loss is negligible")]
        let scale = state.timeframe.duration_secs() as f64 / 300.0;
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss,
                 reason = "scale × time is always a small positive number")]
        let eff_min_elapsed = (self.min_elapsed_secs as f64 * scale) as u64;

        // Must have enough elapsed time for the overshoot to develop.
        if state.time_elapsed_secs < eff_min_elapsed {
            return None;
        }

        // Spot must have moved enough to qualify as an overshoot.
        if state.spot_magnitude < self.min_spot_magnitude {
            return None;
        }

        // Enter the OPPOSITE side — bet on reversal.
        let opposite_ask = state.opposite_ask()?;
        if opposite_ask.as_f64() > self.max_opposite_price {
            return None;
        }

        // Low confidence (0.3) — low win rate but high payoff when it hits.
        let confidence = 0.3;

        Some(EntryDecision {
            side: state.spot_direction.opposite(),
            limit_price: opposite_ask,
            confidence,
            strategy_id: StrategyId::MeanReversion,
            label: self.label,
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
        opposite_ask: Option<f64>,
    ) -> MarketState {
        let duration = 3600_u64; // Hour1
        let remaining = duration.saturating_sub(elapsed);
        let (ask_up, ask_down) = match direction {
            // If spot moved Up, opposite is Down — the cheap contrarian bet
            Side::Up => (Some(0.80), opposite_ask),
            Side::Down => (opposite_ask, Some(0.80)),
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
            time_remaining_secs: remaining,
            contract_ask_up: ask_up.and_then(ContractPrice::new),
            contract_ask_down: ask_down.and_then(ContractPrice::new),
            contract_bid_up: ask_up.and_then(|v| ContractPrice::new(v - 0.02)),
            contract_bid_down: ask_down.and_then(|v| ContractPrice::new(v - 0.02)),
            orderbook_imbalance: None,
        }
    }

    #[test]
    fn fires_contrarian_after_overshoot() {
        let strategy = MeanReversion::new(180, 0.005, 0.30);
        // Hour1: eff_min_elapsed = 180 * 3600/300 = 2160. elapsed=2500 >= 2160.
        // Spot moved Up with magnitude=0.01, opposite (Down) ask=0.25 <= 0.30.
        let state = make_state(Side::Up, 0.01, 2500, Some(0.25));
        let d = strategy.evaluate(&state);
        assert!(d.is_some(), "expected mean reversion to fire");
        let d = d.expect("checked above");
        // Should enter the OPPOSITE side
        assert_eq!(d.side, Side::Down);
        assert_eq!(d.strategy_id, StrategyId::MeanReversion);
        assert!((d.confidence - 0.3).abs() < 1e-10);
    }

    #[test]
    fn enters_opposite_side_when_spot_is_down() {
        let strategy = MeanReversion::new(180, 0.005, 0.30);
        // Spot moved Down, should enter Up
        let state = make_state(Side::Down, 0.01, 2500, Some(0.25));
        let d = strategy.evaluate(&state);
        assert!(d.is_some());
        assert_eq!(d.expect("checked above").side, Side::Up);
    }

    #[test]
    fn does_not_fire_too_early() {
        let strategy = MeanReversion::new(180, 0.005, 0.30);
        // Hour1: eff_min_elapsed = 2160. elapsed=1000 < 2160.
        let state = make_state(Side::Up, 0.01, 1000, Some(0.25));
        assert!(strategy.evaluate(&state).is_none());
    }

    #[test]
    fn does_not_fire_when_magnitude_too_small() {
        let strategy = MeanReversion::new(180, 0.005, 0.30);
        // magnitude=0.002 < 0.005
        let state = make_state(Side::Up, 0.002, 2500, Some(0.25));
        assert!(strategy.evaluate(&state).is_none());
    }

    #[test]
    fn does_not_fire_when_opposite_price_too_high() {
        let strategy = MeanReversion::new(180, 0.005, 0.30);
        // opposite_ask=0.40 > 0.30
        let state = make_state(Side::Up, 0.01, 2500, Some(0.40));
        assert!(strategy.evaluate(&state).is_none());
    }

    #[test]
    fn does_not_fire_when_no_contract_data() {
        let strategy = MeanReversion::new(180, 0.005, 0.30);
        let state = make_state(Side::Up, 0.01, 2500, None);
        assert!(strategy.evaluate(&state).is_none());
    }

    #[test]
    fn id_is_mean_reversion() {
        let strategy = MeanReversion::new(180, 0.005, 0.30);
        assert_eq!(strategy.id(), StrategyId::MeanReversion);
    }
}
