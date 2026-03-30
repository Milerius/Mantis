//! Exponential Moving Average (EMA) tracker per asset.
//!
//! Provides a higher-timeframe trend filter by tracking fast and slow EMAs
//! for each [`Asset`].  The trend direction is determined by the fast/slow
//! crossover: when the fast EMA is above the slow EMA the trend is [`Up`],
//! below is [`Down`], and within 0.05 % of each other is [`Flat`].

use pm_types::{Asset, TrendDirection};

// ─── EmaState ───────────────────────────────────────────────────────────────

/// Per-asset EMA state.
#[derive(Debug, Clone, Copy)]
struct EmaState {
    fast_ema: f64,
    slow_ema: f64,
    initialized: bool,
}

impl Default for EmaState {
    fn default() -> Self {
        Self {
            fast_ema: 0.0,
            slow_ema: 0.0,
            initialized: false,
        }
    }
}

// ─── EmaTracker ─────────────────────────────────────────────────────────────

/// Tracks fast and slow Exponential Moving Averages for each [`Asset`].
///
/// Feed price ticks via [`update`](Self::update) and query the trend via
/// [`trend`](Self::trend) and [`trend_strength`](Self::trend_strength).
pub struct EmaTracker {
    /// Per-asset EMA state.
    state: [EmaState; Asset::COUNT],
    /// Smoothing factor for the fast EMA: `2 / (fast_period + 1)`.
    fast_alpha: f64,
    /// Smoothing factor for the slow EMA: `2 / (slow_period + 1)`.
    slow_alpha: f64,
}

/// Threshold (fractional) below which fast/slow EMAs are considered equal.
///
/// 0.05 % of the slow EMA value.
const FLAT_THRESHOLD: f64 = 0.0005;

impl EmaTracker {
    /// Create a new tracker with the given fast and slow EMA periods (in ticks).
    ///
    /// # Panics
    ///
    /// Panics if either period is zero.
    #[must_use]
    pub fn new(fast_period: usize, slow_period: usize) -> Self {
        assert!(fast_period > 0, "fast_period must be > 0");
        assert!(slow_period > 0, "slow_period must be > 0");
        #[expect(clippy::cast_precision_loss, reason = "EMA periods are small integers")]
        let fast_alpha = 2.0 / (fast_period as f64 + 1.0);
        #[expect(clippy::cast_precision_loss, reason = "EMA periods are small integers")]
        let slow_alpha = 2.0 / (slow_period as f64 + 1.0);
        Self {
            state: [EmaState::default(); Asset::COUNT],
            fast_alpha,
            slow_alpha,
        }
    }

    /// Update both EMAs for `asset` with a new `price` tick.
    ///
    /// On the first tick for an asset the EMAs are seeded to `price`.
    pub fn update(&mut self, asset: Asset, price: f64) {
        let s = &mut self.state[asset.index()];
        if s.initialized {
            s.fast_ema = price * self.fast_alpha + s.fast_ema * (1.0 - self.fast_alpha);
            s.slow_ema = price * self.slow_alpha + s.slow_ema * (1.0 - self.slow_alpha);
        } else {
            s.fast_ema = price;
            s.slow_ema = price;
            s.initialized = true;
        }
    }

    /// Returns the current trend direction for `asset`, or `None` if no ticks
    /// have been received yet.
    #[must_use]
    pub fn trend(&self, asset: Asset) -> Option<TrendDirection> {
        let s = &self.state[asset.index()];
        if !s.initialized {
            return None;
        }
        let diff = s.fast_ema - s.slow_ema;
        let threshold = s.slow_ema.abs() * FLAT_THRESHOLD;
        if diff.abs() <= threshold {
            Some(TrendDirection::Flat)
        } else if diff > 0.0 {
            Some(TrendDirection::Up)
        } else {
            Some(TrendDirection::Down)
        }
    }

    /// Returns how far apart the fast and slow EMAs are, normalised by the
    /// slow EMA value.
    ///
    /// Returns `0.0` if the asset has not been initialised or the slow EMA is
    /// zero.
    #[must_use]
    pub fn trend_strength(&self, asset: Asset) -> f64 {
        let s = &self.state[asset.index()];
        if !s.initialized || s.slow_ema == 0.0 {
            return 0.0;
        }
        ((s.fast_ema - s.slow_ema) / s.slow_ema).abs()
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rising_prices_produce_up_trend() {
        let mut ema = EmaTracker::new(5, 20);
        // Feed 100 rising prices.
        for i in 0..100 {
            ema.update(Asset::Btc, 100.0 + i as f64);
        }
        assert_eq!(ema.trend(Asset::Btc), Some(TrendDirection::Up));
    }

    #[test]
    fn falling_prices_produce_down_trend() {
        let mut ema = EmaTracker::new(5, 20);
        for i in 0..100 {
            ema.update(Asset::Btc, 200.0 - i as f64);
        }
        assert_eq!(ema.trend(Asset::Btc), Some(TrendDirection::Down));
    }

    #[test]
    fn flat_prices_produce_flat_trend() {
        let mut ema = EmaTracker::new(5, 20);
        for _ in 0..200 {
            ema.update(Asset::Btc, 100.0);
        }
        assert_eq!(ema.trend(Asset::Btc), Some(TrendDirection::Flat));
    }

    #[test]
    fn trend_none_before_any_data() {
        let ema = EmaTracker::new(5, 20);
        assert_eq!(ema.trend(Asset::Btc), None);
    }

    #[test]
    fn trend_strength_increases_with_divergence() {
        let mut ema = EmaTracker::new(5, 20);
        // Seed with flat prices.
        for _ in 0..50 {
            ema.update(Asset::Eth, 100.0);
        }
        let strength_before = ema.trend_strength(Asset::Eth);

        // Now feed rising prices.
        for i in 0..50 {
            ema.update(Asset::Eth, 100.0 + i as f64 * 0.5);
        }
        let strength_after = ema.trend_strength(Asset::Eth);

        assert!(
            strength_after > strength_before,
            "strength should increase as EMAs diverge: before={strength_before}, after={strength_after}"
        );
    }

    #[test]
    fn trend_strength_zero_before_data() {
        let ema = EmaTracker::new(10, 30);
        assert_eq!(ema.trend_strength(Asset::Sol), 0.0);
    }

    #[test]
    fn each_asset_is_independent() {
        let mut ema = EmaTracker::new(5, 20);
        for i in 0..100 {
            ema.update(Asset::Btc, 100.0 + i as f64);
            ema.update(Asset::Eth, 200.0 - i as f64);
        }
        assert_eq!(ema.trend(Asset::Btc), Some(TrendDirection::Up));
        assert_eq!(ema.trend(Asset::Eth), Some(TrendDirection::Down));
    }
}
