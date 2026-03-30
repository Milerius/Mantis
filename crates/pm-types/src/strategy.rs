//! Strategy types for the multi-strategy trading engine.
//!
//! Defines [`StrategyId`], the market snapshot [`MarketState`], and the
//! per-strategy entry signal [`EntryDecision`].

#[cfg(feature = "std")]
use serde::{Deserialize, Serialize};

use crate::{
    asset::{Asset, Side, Timeframe},
    market::WindowId,
    price::{ContractPrice, Price},
};

// ─── StrategyId ──────────────────────────────────────────────────────────────

/// Identifies which strategy produced a given signal or trade record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub enum StrategyId {
    /// Complete-set arbitrage: buy both Up and Down when combined cost < $1.
    CompleteSetArb,
    /// Early directional entry on a strong opening move.
    EarlyDirectional,
    /// Mid-window confirmation of sustained momentum.
    MomentumConfirmation,
    /// Hedge lock: buy the opposite side to cap a losing position.
    HedgeLock,
}

impl core::fmt::Display for StrategyId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::CompleteSetArb => write!(f, "Arb"),
            Self::EarlyDirectional => write!(f, "EarlyDir"),
            Self::MomentumConfirmation => write!(f, "Momentum"),
            Self::HedgeLock => write!(f, "Hedge"),
        }
    }
}

// ─── MarketState ─────────────────────────────────────────────────────────────

/// A complete market snapshot passed to every strategy's `evaluate` method.
///
/// All strategies receive the same snapshot; each picks out the fields it
/// needs and returns an [`EntryDecision`] or `None`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct MarketState {
    /// Underlying asset.
    pub asset: Asset,
    /// Prediction window timeframe.
    pub timeframe: Timeframe,
    /// Identifier of the current window.
    pub window_id: WindowId,
    /// Asset price at window open.
    pub window_open_price: Price,
    /// Most recent spot price.
    pub current_spot: Price,
    /// Absolute fractional price move from open (e.g. 0.02 = 2 %).
    pub spot_magnitude: f64,
    /// Direction of the current spot move relative to window open.
    pub spot_direction: Side,
    /// Seconds elapsed since the window opened.
    pub time_elapsed_secs: u64,
    /// Seconds remaining until the window closes.
    pub time_remaining_secs: u64,
    /// Current ask for the Up contract.
    pub contract_ask_up: Option<ContractPrice>,
    /// Current ask for the Down contract.
    pub contract_ask_down: Option<ContractPrice>,
    /// Current bid for the Up contract.
    pub contract_bid_up: Option<ContractPrice>,
    /// Current bid for the Down contract.
    pub contract_bid_down: Option<ContractPrice>,
    /// Orderbook imbalance at top 5 levels. Range `[-1, 1]`.
    ///
    /// Positive = buy pressure (bullish), negative = sell pressure (bearish).
    /// `None` when no L2 orderbook data is available.
    pub orderbook_imbalance: Option<f64>,
}

impl MarketState {
    /// Ask price for the contract matching the current spot direction.
    ///
    /// Returns `None` if the relevant contract ask is unavailable.
    #[inline]
    #[must_use]
    pub fn direction_ask(&self) -> Option<ContractPrice> {
        match self.spot_direction {
            Side::Up => self.contract_ask_up,
            Side::Down => self.contract_ask_down,
        }
    }

    /// Ask price for the contract opposing the current spot direction.
    ///
    /// Returns `None` if the relevant contract ask is unavailable.
    #[inline]
    #[must_use]
    pub fn opposite_ask(&self) -> Option<ContractPrice> {
        match self.spot_direction {
            Side::Up => self.contract_ask_down,
            Side::Down => self.contract_ask_up,
        }
    }
}

// ─── EntryDecision ───────────────────────────────────────────────────────────

/// A strategy's recommendation to enter a position.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct EntryDecision {
    /// Which contract side to buy.
    pub side: Side,
    /// Limit price at which to submit the order.
    pub limit_price: ContractPrice,
    /// Model confidence in the range `[0.0, 1.0]`.
    pub confidence: f64,
    /// Strategy that produced this decision.
    pub strategy_id: StrategyId,
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    extern crate std;
    use std::string::ToString;

    use super::*;
    use crate::{Price, WindowId};

    fn make_state(direction: Side, ask_up: f64, ask_down: f64) -> MarketState {
        MarketState {
            asset: Asset::Btc,
            timeframe: Timeframe::Hour1,
            window_id: WindowId::new(1),
            window_open_price: Price::new(100.0).expect("valid"),
            current_spot: Price::new(102.0).expect("valid"),
            spot_magnitude: 0.02,
            spot_direction: direction,
            time_elapsed_secs: 600,
            time_remaining_secs: 3000,
            contract_ask_up: ContractPrice::new(ask_up),
            contract_ask_down: ContractPrice::new(ask_down),
            contract_bid_up: ContractPrice::new(ask_up - 0.02),
            contract_bid_down: ContractPrice::new(ask_down - 0.02),
            orderbook_imbalance: None,
        }
    }

    #[test]
    fn strategy_id_display() {
        assert_eq!(StrategyId::CompleteSetArb.to_string(), "Arb");
        assert_eq!(StrategyId::EarlyDirectional.to_string(), "EarlyDir");
        assert_eq!(StrategyId::MomentumConfirmation.to_string(), "Momentum");
        assert_eq!(StrategyId::HedgeLock.to_string(), "Hedge");
    }

    #[test]
    fn direction_ask_up_returns_ask_up() {
        let state = make_state(Side::Up, 0.55, 0.48);
        let ask = state.direction_ask().expect("ask should exist");
        assert!((ask.as_f64() - 0.55).abs() < 1e-10);
    }

    #[test]
    fn direction_ask_down_returns_ask_down() {
        let state = make_state(Side::Down, 0.55, 0.48);
        let ask = state.direction_ask().expect("ask should exist");
        assert!((ask.as_f64() - 0.48).abs() < 1e-10);
    }

    #[test]
    fn opposite_ask_up_returns_ask_down() {
        let state = make_state(Side::Up, 0.55, 0.48);
        let ask = state.opposite_ask().expect("ask should exist");
        assert!((ask.as_f64() - 0.48).abs() < 1e-10);
    }

    #[test]
    fn opposite_ask_down_returns_ask_up() {
        let state = make_state(Side::Down, 0.55, 0.48);
        let ask = state.opposite_ask().expect("ask should exist");
        assert!((ask.as_f64() - 0.55).abs() < 1e-10);
    }

    #[test]
    fn direction_ask_none_when_missing() {
        let mut state = make_state(Side::Up, 0.55, 0.48);
        state.contract_ask_up = None;
        assert!(state.direction_ask().is_none());
    }
}
