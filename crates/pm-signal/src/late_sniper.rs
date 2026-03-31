//! Late-window sniper strategy.
//!
//! Enters in the last seconds of a window when the direction is strongly
//! established and the contract is still not at extreme prices.

use pm_types::{EntryDecision, MarketState, StrategyId, StrategyLabel};

use crate::strategy_trait::Strategy;

// ─── LateWindowSniper ───────────────────────────────────────────────────────

/// Enters late in the window when direction is strongly established.
///
/// Entry conditions (all must be true):
/// - `time_remaining_secs <= eff_max_remaining`
/// - `spot_magnitude >= min_spot_magnitude`
/// - `direction_ask <= max_entry_price`
pub struct LateWindowSniper {
    /// Minimum seconds remaining before this strategy activates (e.g. 60 = last 60s).
    pub max_remaining_secs: u64,
    /// Minimum spot magnitude to confirm strong direction (e.g. 0.002 = 0.2%).
    pub min_spot_magnitude: f64,
    /// Maximum contract ask price — don't buy if already too expensive (e.g. 0.85).
    pub max_entry_price: f64,
    /// Human-readable label to distinguish variants.
    pub label: StrategyLabel,
}

impl LateWindowSniper {
    /// Construct a new [`LateWindowSniper`] strategy.
    #[inline]
    #[must_use]
    pub fn new(max_remaining_secs: u64, min_spot_magnitude: f64, max_entry_price: f64) -> Self {
        Self {
            max_remaining_secs,
            min_spot_magnitude,
            max_entry_price,
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

impl Strategy for LateWindowSniper {
    fn id(&self) -> StrategyId {
        StrategyId::LateWindowSniper
    }

    fn evaluate(&self, state: &MarketState) -> Option<EntryDecision> {
        // Scale time boundaries proportionally to timeframe duration.
        // Configured values are for a 5m (300s) window.
        #[expect(clippy::cast_precision_loss,
                 reason = "duration_secs() is at most a few hours in seconds; precision loss is negligible")]
        let scale = state.timeframe.duration_secs() as f64 / 300.0;
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss,
                 reason = "scale × time is always a small positive number")]
        let eff_max_remaining = (self.max_remaining_secs as f64 * scale) as u64;

        // Only fire in the last portion of the window.
        if state.time_remaining_secs > eff_max_remaining {
            return None;
        }

        // Spot must show strong direction.
        if state.spot_magnitude < self.min_spot_magnitude {
            return None;
        }

        // Contract must be available and not too expensive.
        let ask = state.direction_ask()?;
        if ask.as_f64() > self.max_entry_price {
            return None;
        }

        // High confidence (0.8) — late window with strong momentum is very
        // likely to hold through expiry.
        let confidence = 0.8;

        Some(EntryDecision {
            side: state.spot_direction,
            limit_price: ask,
            confidence,
            strategy_id: StrategyId::LateWindowSniper,
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
        remaining: u64,
        ask_direction: Option<f64>,
    ) -> MarketState {
        let duration = 3600_u64; // Hour1
        let elapsed = duration.saturating_sub(remaining);
        let (ask_up, ask_down) = match direction {
            Side::Up => (ask_direction, Some(0.20)),
            Side::Down => (Some(0.20), ask_direction),
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
    fn fires_in_last_window_with_strong_momentum() {
        let strategy = LateWindowSniper::new(60, 0.002, 0.85);
        // Hour1: eff_max_remaining = 60 * 3600/300 = 720s. remaining=500 <= 720.
        let state = make_state(Side::Up, 0.005, 500, Some(0.70));
        let d = strategy.evaluate(&state);
        assert!(d.is_some(), "expected late sniper to fire");
        let d = d.expect("checked above");
        assert_eq!(d.side, Side::Up);
        assert_eq!(d.strategy_id, StrategyId::LateWindowSniper);
        assert!((d.confidence - 0.8).abs() < 1e-10);
    }

    #[test]
    fn does_not_fire_when_too_much_time_remaining() {
        let strategy = LateWindowSniper::new(60, 0.002, 0.85);
        // Hour1: eff_max_remaining = 720. remaining=1000 > 720.
        let state = make_state(Side::Up, 0.005, 1000, Some(0.70));
        assert!(strategy.evaluate(&state).is_none());
    }

    #[test]
    fn does_not_fire_when_magnitude_too_small() {
        let strategy = LateWindowSniper::new(60, 0.002, 0.85);
        // magnitude=0.001 < 0.002
        let state = make_state(Side::Up, 0.001, 500, Some(0.70));
        assert!(strategy.evaluate(&state).is_none());
    }

    #[test]
    fn does_not_fire_when_price_too_high() {
        let strategy = LateWindowSniper::new(60, 0.002, 0.85);
        // ask=0.90 > 0.85
        let state = make_state(Side::Up, 0.005, 500, Some(0.90));
        assert!(strategy.evaluate(&state).is_none());
    }

    #[test]
    fn does_not_fire_when_no_contract_data() {
        let strategy = LateWindowSniper::new(60, 0.002, 0.85);
        let state = make_state(Side::Up, 0.005, 500, None);
        assert!(strategy.evaluate(&state).is_none());
    }

    #[test]
    fn id_is_late_window_sniper() {
        let strategy = LateWindowSniper::new(60, 0.002, 0.85);
        assert_eq!(strategy.id(), StrategyId::LateWindowSniper);
    }
}
