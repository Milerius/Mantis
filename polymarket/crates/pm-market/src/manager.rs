//! Market manager: combines scanner results and orderbook state.
//!
//! [`MarketManager`] is the single point of contact for the rest of the bot:
//! it maintains the live map of known markets and their current orderbook
//! snapshots, reconciling periodic scanner refreshes with real-time WebSocket
//! updates.

use std::collections::HashMap;
use std::time::Duration;

use tracing::info;

use crate::orderbook::{OrderbookSnapshot, OrderbookTracker};
use crate::scanner::MarketInfo;

// ─── MarketManager ───────────────────────────────────────────────────────────

/// Manages live market state: discovered markets + their orderbook snapshots.
///
/// Call [`update_markets`](Self::update_markets) after each scanner poll and
/// [`OrderbookTracker::update`] (via [`orderbook_tracker_mut`](Self::orderbook_tracker_mut))
/// for each WebSocket message.
pub struct MarketManager {
    /// How often the scanner should be called to refresh market discovery.
    pub scanner_interval: Duration,
    /// All known active markets, keyed by condition_id.
    markets: HashMap<String, MarketInfo>,
    /// Live orderbook snapshots.
    orderbooks: OrderbookTracker,
}

impl MarketManager {
    /// Create a new manager with the given scanner poll interval.
    #[must_use]
    pub fn new(scanner_interval: Duration) -> Self {
        Self {
            scanner_interval,
            markets: HashMap::new(),
            orderbooks: OrderbookTracker::new(),
        }
    }

    /// Get the current orderbook snapshot for a market by condition ID.
    ///
    /// Returns `None` if the market is unknown or has no quotes yet.
    #[must_use]
    pub fn orderbook(&self, condition_id: &str) -> Option<&OrderbookSnapshot> {
        self.orderbooks.get(condition_id)
    }

    /// Iterate over all currently known active markets.
    pub fn active_markets(&self) -> impl Iterator<Item = &MarketInfo> {
        self.markets.values()
    }

    /// Replace the active market set with the latest scanner results.
    ///
    /// New markets are registered with the orderbook tracker. Stale markets
    /// (no longer returned by the scanner) are removed from both maps.
    pub fn update_markets(&mut self, markets: Vec<MarketInfo>) {
        // Build the new condition_id set.
        let new_ids: std::collections::HashSet<String> =
            markets.iter().map(|m| m.condition_id.clone()).collect();

        // Remove stale markets no longer returned by the scanner.
        self.markets.retain(|id, _| new_ids.contains(id.as_str()));

        // Insert or update markets; register any truly new ones with the tracker.
        for market in markets {
            if !self.markets.contains_key(&market.condition_id) {
                info!(
                    condition_id = %market.condition_id,
                    asset = %market.asset,
                    timeframe = %market.timeframe,
                    "registering new market"
                );
                self.orderbooks.register_market(
                    &market.condition_id,
                    &market.token_id_up,
                    &market.token_id_down,
                );
            }
            self.markets.insert(market.condition_id.clone(), market);
        }
    }

    /// Provide mutable access to the underlying [`OrderbookTracker`].
    ///
    /// Use this to route WebSocket update messages into the tracker.
    pub fn orderbook_tracker_mut(&mut self) -> &mut OrderbookTracker {
        &mut self.orderbooks
    }

    /// Number of currently tracked markets.
    #[must_use]
    pub fn market_count(&self) -> usize {
        self.markets.len()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use pm_types::{Asset, Timeframe};

    use super::*;

    fn make_market(condition_id: &str, asset: Asset, timeframe: Timeframe) -> MarketInfo {
        MarketInfo {
            slug: format!("{}-updown", condition_id),
            condition_id: condition_id.to_owned(),
            token_id_up: format!("{}-up", condition_id),
            token_id_down: format!("{}-down", condition_id),
            asset,
            timeframe,
            end_date: "2024-01-15T12:00:00Z".to_owned(),
        }
    }

    #[test]
    fn manager_starts_empty() {
        let mgr = MarketManager::new(Duration::from_secs(60));
        assert_eq!(mgr.market_count(), 0);
        assert!(mgr.active_markets().next().is_none());
    }

    #[test]
    fn update_markets_adds_new_entries() {
        let mut mgr = MarketManager::new(Duration::from_secs(60));
        mgr.update_markets(vec![
            make_market("cond_1", Asset::Btc, Timeframe::Min15),
            make_market("cond_2", Asset::Eth, Timeframe::Hour1),
        ]);

        assert_eq!(mgr.market_count(), 2);
    }

    #[test]
    fn update_markets_removes_stale_entries() {
        let mut mgr = MarketManager::new(Duration::from_secs(60));
        mgr.update_markets(vec![
            make_market("cond_1", Asset::Btc, Timeframe::Min15),
            make_market("cond_2", Asset::Eth, Timeframe::Hour1),
        ]);

        // Second scan only returns one market.
        mgr.update_markets(vec![make_market("cond_1", Asset::Btc, Timeframe::Min15)]);

        assert_eq!(mgr.market_count(), 1);
        assert!(mgr.active_markets().any(|m| m.condition_id == "cond_1"));
    }

    #[test]
    fn update_markets_idempotent_for_existing() {
        let mut mgr = MarketManager::new(Duration::from_secs(60));
        mgr.update_markets(vec![make_market("cond_1", Asset::Btc, Timeframe::Min15)]);
        mgr.update_markets(vec![make_market("cond_1", Asset::Btc, Timeframe::Min15)]);
        assert_eq!(mgr.market_count(), 1);
    }

    #[test]
    fn orderbook_returns_none_for_unknown_market() {
        let mgr = MarketManager::new(Duration::from_secs(60));
        assert!(mgr.orderbook("nonexistent").is_none());
    }

    #[test]
    fn orderbook_returns_snapshot_after_update() {
        let mut mgr = MarketManager::new(Duration::from_secs(60));
        mgr.update_markets(vec![make_market("cond_1", Asset::Btc, Timeframe::Min15)]);

        mgr.orderbook_tracker_mut()
            .update("cond_1-up", "SELL", 0.60, 1_000);

        let snap = mgr.orderbook("cond_1").expect("snapshot should exist");
        let ask = snap.ask_up.expect("ask_up should be populated");
        assert!((ask.as_f64() - 0.60).abs() < 1e-10);
    }

    #[test]
    fn scanner_interval_is_stored() {
        let mgr = MarketManager::new(Duration::from_secs(120));
        assert_eq!(mgr.scanner_interval, Duration::from_secs(120));
    }
}
