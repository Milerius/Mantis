//! Market data structures: identifiers, ticks, windows, and signals.
//!
//! These types flow through the oracle → signal pipeline. All are `Copy + Clone`
//! so they can be passed through channels without heap allocation.

#[cfg(feature = "std")]
use serde::{Deserialize, Serialize};

use crate::{
    asset::{Asset, ExchangeSource, Side, Timeframe},
    price::{ContractPrice, Edge, Price},
};

// ─── Identifiers ─────────────────────────────────────────────────────────────

/// Unique identifier for a prediction window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
#[repr(transparent)]
pub struct WindowId(u64);

impl WindowId {
    /// Construct a [`WindowId`] from a raw `u64`.
    #[inline]
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Return the raw inner `u64`.
    #[inline]
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl core::fmt::Display for WindowId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "W{}", self.0)
    }
}

/// Unique identifier for an order placed on Polymarket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
#[repr(transparent)]
pub struct OrderId(u64);

impl OrderId {
    /// Construct an [`OrderId`] from a raw `u64`.
    #[inline]
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Return the raw inner `u64`.
    #[inline]
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl core::fmt::Display for OrderId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "O{}", self.0)
    }
}

// ─── Tick ────────────────────────────────────────────────────────────────────

/// A single price tick from an exchange feed.
///
/// `timestamp_ms` is Unix epoch time in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct Tick {
    /// The underlying asset this tick refers to.
    pub asset: Asset,
    /// Mid price at the time of the tick.
    pub price: Price,
    /// Timestamp in milliseconds since Unix epoch.
    pub timestamp_ms: u64,
    /// Exchange that produced this tick.
    pub source: ExchangeSource,
}

// ─── Window ──────────────────────────────────────────────────────────────────

/// A prediction window: a time-bounded price movement bet.
///
/// The window opens at `open_time_ms` and closes at `close_time_ms`.
/// At close, the outcome is determined by comparing the closing price with
/// `open_price`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct Window {
    /// Unique identifier for this window.
    pub id: WindowId,
    /// Underlying asset.
    pub asset: Asset,
    /// Candle timeframe this window corresponds to.
    pub timeframe: Timeframe,
    /// Window open time, milliseconds since Unix epoch.
    pub open_time_ms: u64,
    /// Window close time, milliseconds since Unix epoch.
    pub close_time_ms: u64,
    /// Asset price at window open.
    pub open_price: Price,
}

impl Window {
    /// Seconds remaining until this window closes, given `now_ms`.
    ///
    /// Returns `0` if the window has already closed.
    #[inline]
    #[must_use]
    pub fn time_remaining_secs(&self, now_ms: u64) -> u64 {
        if now_ms >= self.close_time_ms {
            0
        } else {
            (self.close_time_ms - now_ms) / 1_000
        }
    }

    /// Absolute percentage move from open to `current`.
    ///
    /// Returns `0.0` if `open_price` is zero to avoid division by zero.
    #[inline]
    #[must_use]
    pub fn magnitude(&self, current: Price) -> f64 {
        let open = self.open_price.as_f64();
        if open == 0.0 {
            return 0.0;
        }
        ((current.as_f64() - open) / open).abs()
    }

    /// Direction of price movement from open to `current`.
    ///
    /// Returns [`Side::Up`] if `current >= open_price` (including flat),
    /// and [`Side::Down`] otherwise.
    #[inline]
    #[must_use]
    pub fn direction(&self, current: Price) -> Side {
        if current.as_f64() >= self.open_price.as_f64() {
            Side::Up
        } else {
            Side::Down
        }
    }
}

// ─── Signal ──────────────────────────────────────────────────────────────────

/// A trading signal produced by the signal engine for a specific window.
///
/// Encapsulates everything needed by the executor to decide whether and how
/// much to bet on a Polymarket outcome.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct Signal {
    /// The window this signal refers to.
    pub window_id: WindowId,
    /// Predicted outcome direction.
    pub side: Side,
    /// Model's fair-value estimate for the YES contract.
    pub fair_value: ContractPrice,
    /// Current market price of the YES contract.
    pub market_price: ContractPrice,
    /// Pricing edge: `fair_value - market_price` (or its complement for Down).
    pub edge: Edge,
    /// Magnitude of expected price move (absolute fractional change).
    pub magnitude: f64,
    /// Seconds remaining in the window at signal generation time.
    pub time_remaining_secs: u64,
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_window(open_price: f64, open_time_ms: u64, close_time_ms: u64) -> Window {
        Window {
            id: WindowId::new(1),
            asset: Asset::Btc,
            timeframe: Timeframe::Hour1,
            open_time_ms,
            close_time_ms,
            open_price: Price::new(open_price).expect("valid open price"),
        }
    }

    // time_remaining_secs

    #[test]
    fn time_remaining_mid_window() {
        // Window: 0ms → 3_600_000ms (1 hour).
        let w = make_window(100.0, 0, 3_600_000);
        // At t = 1_800_000ms (half way through), 1800s remain.
        assert_eq!(w.time_remaining_secs(1_800_000), 1_800);
    }

    #[test]
    fn time_remaining_after_close() {
        let w = make_window(100.0, 0, 3_600_000);
        assert_eq!(w.time_remaining_secs(3_600_000), 0);
        assert_eq!(w.time_remaining_secs(5_000_000), 0);
    }

    #[test]
    fn time_remaining_at_open() {
        let w = make_window(100.0, 0, 3_600_000);
        assert_eq!(w.time_remaining_secs(0), 3_600);
    }

    // magnitude

    #[test]
    fn magnitude_up_move() {
        let w = make_window(100.0, 0, 3_600_000);
        let current = Price::new(110.0).expect("valid price");
        // |110 - 100| / 100 = 0.10
        let mag = w.magnitude(current);
        assert!((mag - 0.10).abs() < 1e-10, "expected 0.10 got {mag}");
    }

    #[test]
    fn magnitude_down_move() {
        let w = make_window(100.0, 0, 3_600_000);
        let current = Price::new(90.0).expect("valid price");
        // |90 - 100| / 100 = 0.10
        let mag = w.magnitude(current);
        assert!((mag - 0.10).abs() < 1e-10, "expected 0.10 got {mag}");
    }

    #[test]
    fn magnitude_no_move() {
        let w = make_window(100.0, 0, 3_600_000);
        let current = Price::new(100.0).expect("valid price");
        assert_eq!(w.magnitude(current), 0.0);
    }

    #[test]
    fn magnitude_zero_open_price() {
        let w = make_window(0.0, 0, 3_600_000);
        let current = Price::new(50.0).expect("valid price");
        assert_eq!(w.magnitude(current), 0.0);
    }

    // direction

    #[test]
    fn direction_up() {
        let w = make_window(100.0, 0, 3_600_000);
        let current = Price::new(101.0).expect("valid price");
        assert_eq!(w.direction(current), Side::Up);
    }

    #[test]
    fn direction_down() {
        let w = make_window(100.0, 0, 3_600_000);
        let current = Price::new(99.0).expect("valid price");
        assert_eq!(w.direction(current), Side::Down);
    }

    #[test]
    fn direction_flat_is_up() {
        let w = make_window(100.0, 0, 3_600_000);
        let current = Price::new(100.0).expect("valid price");
        assert_eq!(w.direction(current), Side::Up);
    }

    // Identifier display

    #[test]
    fn window_id_display() {
        assert_eq!(WindowId::new(42).to_string(), "W42");
    }

    #[test]
    fn order_id_display() {
        assert_eq!(OrderId::new(7).to_string(), "O7");
    }
}
