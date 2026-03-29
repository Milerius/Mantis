//! Trade execution types: fills, positions, orders, and trade records.
//!
//! These types record what the executor did and how it turned out. The bookkeeper
//! consumes [`TradeRecord`] values to build PnL reports.

#[cfg(feature = "std")]
use serde::{Deserialize, Serialize};

use crate::{
    asset::{Asset, Side},
    market::{OrderId, WindowId},
    price::{ContractPrice, Pnl},
};

// ─── Fill ────────────────────────────────────────────────────────────────────

/// A confirmed order fill returned by the Polymarket CLOB.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct Fill {
    /// The order that was filled.
    pub order_id: OrderId,
    /// Actual fill price in `[0, 1]`.
    pub fill_price: ContractPrice,
    /// USDC size filled.
    pub size_usdc: f64,
    /// Timestamp of fill in milliseconds since Unix epoch.
    pub timestamp_ms: u64,
}

// ─── OpenPosition ────────────────────────────────────────────────────────────

/// An open Polymarket position held by the bot.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct OpenPosition {
    /// The window this position is for.
    pub window_id: WindowId,
    /// Underlying asset.
    pub asset: Asset,
    /// Direction of the bet.
    pub side: Side,
    /// Average entry price across all fills.
    pub avg_entry: ContractPrice,
    /// Total USDC allocated to this position.
    pub size_usdc: f64,
    /// Time the position was opened, milliseconds since Unix epoch.
    pub opened_at_ms: u64,
}

// ─── SizedOrder ──────────────────────────────────────────────────────────────

/// A sized order ready to be submitted to the CLOB.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct SizedOrder {
    /// The window this order is for.
    pub window_id: WindowId,
    /// Underlying asset.
    pub asset: Asset,
    /// Direction of the bet.
    pub side: Side,
    /// Limit price in `[0, 1]`.
    pub limit_price: ContractPrice,
    /// USDC size to submit.
    pub size_usdc: f64,
    /// Reason this order was generated.
    pub reason: OrderReason,
}

// ─── OrderReason ─────────────────────────────────────────────────────────────

/// Why the executor generated this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub enum OrderReason {
    /// New signal with sufficient edge.
    NewSignal,
    /// Adding to an existing position with improved edge.
    ScaleIn,
    /// Closing a position early (stop-loss or take-profit).
    EarlyClose,
    /// Closing a position at window expiry.
    ExpiryClose,
}

impl core::fmt::Display for OrderReason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NewSignal => write!(f, "NewSignal"),
            Self::ScaleIn => write!(f, "ScaleIn"),
            Self::EarlyClose => write!(f, "EarlyClose"),
            Self::ExpiryClose => write!(f, "ExpiryClose"),
        }
    }
}

// ─── Rejection ───────────────────────────────────────────────────────────────

/// Reason a signal was rejected by the executor without placing an order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub enum Rejection {
    /// Edge was below the configured minimum threshold.
    EdgeTooSmall,
    /// Position would breach the per-position size limit.
    PositionSizeLimitBreached,
    /// Total exposure would breach the configured maximum.
    TotalExposureLimitBreached,
    /// Daily loss limit has been hit; no new trades allowed today.
    DailyLossLimitHit,
    /// The window has too little time remaining to be worth betting.
    WindowExpiringSoon,
    /// The market was not liquid enough to fill at an acceptable price.
    InsufficientLiquidity,
}

impl core::fmt::Display for Rejection {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EdgeTooSmall => write!(f, "EdgeTooSmall"),
            Self::PositionSizeLimitBreached => write!(f, "PositionSizeLimitBreached"),
            Self::TotalExposureLimitBreached => write!(f, "TotalExposureLimitBreached"),
            Self::DailyLossLimitHit => write!(f, "DailyLossLimitHit"),
            Self::WindowExpiringSoon => write!(f, "WindowExpiringSoon"),
            Self::InsufficientLiquidity => write!(f, "InsufficientLiquidity"),
        }
    }
}

// ─── TradeRecord ─────────────────────────────────────────────────────────────

/// A closed trade with its final PnL.
///
/// Created by the bookkeeper when a position is fully closed.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct TradeRecord {
    /// The window the trade was for.
    pub window_id: WindowId,
    /// Underlying asset.
    pub asset: Asset,
    /// Direction of the bet.
    pub side: Side,
    /// Average entry price.
    pub entry_price: ContractPrice,
    /// Average exit price.
    pub exit_price: ContractPrice,
    /// USDC size traded.
    pub size_usdc: f64,
    /// Realised PnL after fees.
    pub pnl: Pnl,
    /// Time the position was opened, milliseconds since Unix epoch.
    pub opened_at_ms: u64,
    /// Time the position was closed, milliseconds since Unix epoch.
    pub closed_at_ms: u64,
    /// Reason the closing order was placed.
    pub close_reason: OrderReason,
}

impl TradeRecord {
    /// Returns `true` if the trade was profitable (`pnl > 0`).
    #[inline]
    #[must_use]
    pub fn is_win(&self) -> bool {
        self.pnl.as_f64() > 0.0
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    extern crate std;
    use std::string::ToString;

    use super::*;

    fn make_record(pnl: f64) -> TradeRecord {
        TradeRecord {
            window_id: WindowId::new(1),
            asset: Asset::Btc,
            side: Side::Up,
            entry_price: ContractPrice::new(0.45).expect("valid price"),
            exit_price: ContractPrice::new(0.80).expect("valid price"),
            size_usdc: 25.0,
            pnl: Pnl::new(pnl).expect("finite pnl"),
            opened_at_ms: 0,
            closed_at_ms: 3_600_000,
            close_reason: OrderReason::ExpiryClose,
        }
    }

    #[test]
    fn trade_record_is_win_positive_pnl() {
        let r = make_record(8.75);
        assert!(r.is_win());
    }

    #[test]
    fn trade_record_is_not_win_negative_pnl() {
        let r = make_record(-3.50);
        assert!(!r.is_win());
    }

    #[test]
    fn trade_record_is_not_win_zero_pnl() {
        let r = make_record(0.0);
        assert!(!r.is_win());
    }

    #[test]
    fn rejection_display() {
        assert_eq!(Rejection::EdgeTooSmall.to_string(), "EdgeTooSmall");
        assert_eq!(
            Rejection::TotalExposureLimitBreached.to_string(),
            "TotalExposureLimitBreached"
        );
    }

    #[test]
    fn order_reason_display() {
        assert_eq!(OrderReason::NewSignal.to_string(), "NewSignal");
        assert_eq!(OrderReason::ExpiryClose.to_string(), "ExpiryClose");
    }
}
