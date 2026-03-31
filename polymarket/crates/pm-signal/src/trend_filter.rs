//! Pre-filter that prevents trading against the prevailing higher-timeframe trend.
//!
//! This is NOT a strategy — it wraps strategy evaluation by rejecting entry
//! decisions whose direction conflicts with the EMA-derived trend.

use pm_types::{Side, TrendDirection};

// ─── TrendFilter ────────────────────────────────────────────────────────────

/// Configuration-driven filter that skips trades opposing the prevailing trend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrendFilter {
    /// If `true`, skip trades where the trend conflicts with the signal direction.
    pub require_trend_alignment: bool,
    /// Minimum trend strength to consider the trend "established" (e.g. `0.0005` = 0.05 %).
    pub min_trend_strength: f64,
}

impl TrendFilter {
    /// Returns `true` if the trade should be **skipped** (filtered out).
    ///
    /// # Logic
    ///
    /// - If `!require_trend_alignment` — never skip.
    /// - If `trend` is `None` (not enough data) — allow trading.
    /// - If trend is `Flat` **and** strength < `min_trend_strength` — skip (no clear trend).
    /// - If trend direction does not match `signal_side` — skip (trading against trend).
    /// - Otherwise — allow.
    #[must_use]
    pub fn should_skip(
        &self,
        signal_side: Side,
        trend: Option<TrendDirection>,
        strength: f64,
    ) -> bool {
        if !self.require_trend_alignment {
            return false;
        }

        let Some(direction) = trend else {
            // Not enough data to determine trend — allow the trade.
            return false;
        };

        match direction {
            TrendDirection::Flat => {
                // Flat trend with weak strength — no conviction, skip.
                strength < self.min_trend_strength
            }
            TrendDirection::Up => signal_side != Side::Up,
            TrendDirection::Down => signal_side != Side::Down,
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn active_filter() -> TrendFilter {
        TrendFilter {
            require_trend_alignment: true,
            min_trend_strength: 0.0005,
        }
    }

    fn disabled_filter() -> TrendFilter {
        TrendFilter {
            require_trend_alignment: false,
            min_trend_strength: 0.0005,
        }
    }

    #[test]
    fn disabled_filter_never_skips() {
        let f = disabled_filter();
        assert!(!f.should_skip(Side::Up, Some(TrendDirection::Down), 0.01));
        assert!(!f.should_skip(Side::Down, Some(TrendDirection::Up), 0.01));
        assert!(!f.should_skip(Side::Up, None, 0.0));
    }

    #[test]
    fn none_trend_allows_trading() {
        let f = active_filter();
        assert!(!f.should_skip(Side::Up, None, 0.0));
        assert!(!f.should_skip(Side::Down, None, 0.0));
    }

    #[test]
    fn flat_trend_weak_strength_skips() {
        let f = active_filter();
        // Flat with strength below threshold → skip.
        assert!(f.should_skip(Side::Up, Some(TrendDirection::Flat), 0.0001));
        assert!(f.should_skip(Side::Down, Some(TrendDirection::Flat), 0.0));
    }

    #[test]
    fn flat_trend_strong_enough_allows() {
        let f = active_filter();
        // Flat with strength at or above threshold → allow.
        assert!(!f.should_skip(Side::Up, Some(TrendDirection::Flat), 0.0005));
        assert!(!f.should_skip(Side::Down, Some(TrendDirection::Flat), 0.001));
    }

    #[test]
    fn aligned_trend_allows() {
        let f = active_filter();
        assert!(!f.should_skip(Side::Up, Some(TrendDirection::Up), 0.01));
        assert!(!f.should_skip(Side::Down, Some(TrendDirection::Down), 0.01));
    }

    #[test]
    fn opposing_trend_skips() {
        let f = active_filter();
        assert!(f.should_skip(Side::Up, Some(TrendDirection::Down), 0.01));
        assert!(f.should_skip(Side::Down, Some(TrendDirection::Up), 0.01));
    }
}
