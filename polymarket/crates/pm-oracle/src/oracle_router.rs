//! Oracle router: merges and deduplicates ticks from multiple sources.
//!
//! [`OracleRouter`] tracks the last-seen timestamp per asset. Ticks whose
//! timestamp is equal to or earlier than the previously seen timestamp for
//! that asset are dropped, so the first tick per `(asset, millisecond)` wins.

use pm_types::{Asset, Tick};

// ─── OracleRouter ────────────────────────────────────────────────────────────

/// Deduplicates [`Tick`]s from multiple exchange feeds.
///
/// For each asset, only the first tick at a given millisecond timestamp is
/// forwarded; duplicates and out-of-order ticks are filtered.
pub struct OracleRouter {
    /// Last-seen timestamp (ms) per asset, indexed by [`Asset::index`].
    last_seen: [u64; Asset::COUNT],
}

impl OracleRouter {
    /// Create a new [`OracleRouter`] with no history.
    #[must_use]
    pub fn new() -> Self {
        Self {
            last_seen: [0; Asset::COUNT],
        }
    }

    /// Process an incoming tick.
    ///
    /// Returns `Some(tick)` if this is the first tick for `(asset, timestamp_ms)`,
    /// or `None` if it is a duplicate or earlier than the last seen tick for
    /// that asset.
    #[must_use]
    pub fn process(&mut self, tick: Tick) -> Option<Tick> {
        let idx = tick.asset.index();
        if tick.timestamp_ms <= self.last_seen[idx] {
            return None;
        }
        self.last_seen[idx] = tick.timestamp_ms;
        Some(tick)
    }
}

impl Default for OracleRouter {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test helpers use expect for conciseness")]
mod tests {
    use pm_types::{ExchangeSource, Price};

    use super::*;

    fn make_tick(asset: Asset, timestamp_ms: u64, source: ExchangeSource) -> Tick {
        Tick {
            asset,
            price: Price::new(100.0).expect("valid price"),
            timestamp_ms,
            source,
        }
    }

    #[test]
    fn first_tick_passes() {
        let mut router = OracleRouter::new();
        let tick = make_tick(Asset::Btc, 1_000, ExchangeSource::Binance);
        assert!(router.process(tick).is_some());
    }

    #[test]
    fn duplicate_same_asset_same_ms_filtered() {
        let mut router = OracleRouter::new();
        let tick_a = make_tick(Asset::Btc, 1_000, ExchangeSource::Binance);
        let tick_b = make_tick(Asset::Btc, 1_000, ExchangeSource::Okx);
        assert!(router.process(tick_a).is_some());
        // Second tick at the same millisecond for the same asset is dropped.
        assert!(router.process(tick_b).is_none());
    }

    #[test]
    fn different_asset_passes_independently() {
        let mut router = OracleRouter::new();
        let btc = make_tick(Asset::Btc, 1_000, ExchangeSource::Binance);
        let eth = make_tick(Asset::Eth, 1_000, ExchangeSource::Binance);
        assert!(router.process(btc).is_some());
        // ETH has its own counter — same timestamp is fine.
        assert!(router.process(eth).is_some());
    }

    #[test]
    fn later_timestamp_passes() {
        let mut router = OracleRouter::new();
        let tick_a = make_tick(Asset::Btc, 1_000, ExchangeSource::Binance);
        let tick_b = make_tick(Asset::Btc, 2_000, ExchangeSource::Binance);
        assert!(router.process(tick_a).is_some());
        assert!(router.process(tick_b).is_some());
    }

    #[test]
    fn earlier_timestamp_filtered() {
        let mut router = OracleRouter::new();
        let tick_a = make_tick(Asset::Btc, 2_000, ExchangeSource::Binance);
        let tick_b = make_tick(Asset::Btc, 1_000, ExchangeSource::Binance);
        assert!(router.process(tick_a).is_some());
        // Out-of-order (earlier) tick is dropped.
        assert!(router.process(tick_b).is_none());
    }

    #[test]
    fn all_assets_tracked_independently() {
        let mut router = OracleRouter::new();
        for asset in Asset::ALL {
            let tick = make_tick(asset, 5_000, ExchangeSource::Binance);
            assert!(
                router.process(tick).is_some(),
                "first tick for {asset:?} should pass"
            );
        }
        for asset in Asset::ALL {
            let dup = make_tick(asset, 5_000, ExchangeSource::Okx);
            assert!(
                router.process(dup).is_none(),
                "duplicate for {asset:?} should be filtered"
            );
        }
    }

    #[test]
    fn returned_tick_is_original() {
        let mut router = OracleRouter::new();
        let tick = make_tick(Asset::Sol, 9_999, ExchangeSource::Okx);
        let result = router.process(tick).expect("first tick should pass");
        assert_eq!(result.asset, Asset::Sol);
        assert_eq!(result.timestamp_ms, 9_999);
        assert_eq!(result.source, ExchangeSource::Okx);
    }
}
