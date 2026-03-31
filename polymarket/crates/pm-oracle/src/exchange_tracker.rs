//! Tracks the latest spot price per (asset, exchange) for cross-exchange confirmation.

use pm_types::asset::ExchangeSource;
use pm_types::{Asset, Price};
use pm_types::market::Tick;

const EXCHANGE_COUNT: usize = 2;

pub struct ExchangePriceTracker {
    prices: [[Option<(Price, u64)>; EXCHANGE_COUNT]; Asset::COUNT],
}

impl ExchangePriceTracker {
    pub fn new() -> Self {
        Self { prices: [[None; EXCHANGE_COUNT]; Asset::COUNT] }
    }

    pub fn update(&mut self, tick: &Tick) {
        let ai = tick.asset.index();
        let ei = match tick.source { ExchangeSource::Binance => 0, ExchangeSource::Okx => 1 };
        self.prices[ai][ei] = Some((tick.price, tick.timestamp_ms));
    }

    pub fn binance_price(&self, asset: Asset) -> Option<Price> {
        self.prices[asset.index()][0].map(|(p, _)| p)
    }

    pub fn okx_price(&self, asset: Asset) -> Option<Price> {
        self.prices[asset.index()][1].map(|(p, _)| p)
    }

    pub fn exchanges_agree(&self, asset: Asset, reference: Price) -> Option<bool> {
        let bp = self.binance_price(asset)?;
        let op = self.okx_price(asset)?;
        let r = reference.as_f64();
        Some((bp.as_f64() >= r) == (op.as_f64() >= r))
    }
}

impl Default for ExchangePriceTracker {
    fn default() -> Self { Self::new() }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test helpers use expect for conciseness")]
mod tests {
    use super::*;

    fn p(v: f64) -> Price {
        Price::new(v).expect("valid price")
    }

    fn binance_tick(asset: Asset, price: f64, ts: u64) -> Tick {
        Tick { asset, price: p(price), timestamp_ms: ts, source: ExchangeSource::Binance }
    }

    fn okx_tick(asset: Asset, price: f64, ts: u64) -> Tick {
        Tick { asset, price: p(price), timestamp_ms: ts, source: ExchangeSource::Okx }
    }

    #[test]
    fn update_and_get_binance_price() {
        let mut tracker = ExchangePriceTracker::new();
        tracker.update(&binance_tick(Asset::Btc, 50_000.0, 1_000));
        let price = tracker.binance_price(Asset::Btc).expect("binance price set");
        assert!((price.as_f64() - 50_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn update_and_get_okx_price() {
        let mut tracker = ExchangePriceTracker::new();
        tracker.update(&okx_tick(Asset::Eth, 3_000.0, 2_000));
        let price = tracker.okx_price(Asset::Eth).expect("okx price set");
        assert!((price.as_f64() - 3_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn update_overwrites_previous_price() {
        let mut tracker = ExchangePriceTracker::new();
        tracker.update(&binance_tick(Asset::Sol, 100.0, 1_000));
        tracker.update(&binance_tick(Asset::Sol, 120.0, 2_000));
        let price = tracker.binance_price(Asset::Sol).expect("price updated");
        assert!((price.as_f64() - 120.0).abs() < f64::EPSILON);
    }

    #[test]
    fn assets_are_independent_in_tracker() {
        let mut tracker = ExchangePriceTracker::new();
        tracker.update(&binance_tick(Asset::Btc, 50_000.0, 1_000));
        tracker.update(&binance_tick(Asset::Eth, 3_000.0, 1_000));
        assert!(tracker.binance_price(Asset::Sol).is_none());
        assert!(tracker.binance_price(Asset::Xrp).is_none());
        assert!((tracker.binance_price(Asset::Btc).expect("btc").as_f64() - 50_000.0).abs() < f64::EPSILON);
        assert!((tracker.binance_price(Asset::Eth).expect("eth").as_f64() - 3_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn exchanges_agree_both_above_reference() {
        let mut tracker = ExchangePriceTracker::new();
        tracker.update(&binance_tick(Asset::Btc, 50_100.0, 1_000));
        tracker.update(&okx_tick(Asset::Btc, 50_200.0, 1_000));
        let reference = p(50_000.0);
        let agree = tracker.exchanges_agree(Asset::Btc, reference).expect("both exchanges have data");
        assert!(agree, "both exchanges are above reference, should agree");
    }

    #[test]
    fn exchanges_agree_both_below_reference() {
        let mut tracker = ExchangePriceTracker::new();
        tracker.update(&binance_tick(Asset::Btc, 49_800.0, 1_000));
        tracker.update(&okx_tick(Asset::Btc, 49_900.0, 1_000));
        let reference = p(50_000.0);
        let agree = tracker.exchanges_agree(Asset::Btc, reference).expect("both exchanges have data");
        assert!(agree, "both exchanges are below reference, should agree");
    }

    #[test]
    fn exchanges_disagree_one_above_one_below() {
        let mut tracker = ExchangePriceTracker::new();
        tracker.update(&binance_tick(Asset::Eth, 3_100.0, 1_000));
        tracker.update(&okx_tick(Asset::Eth, 2_900.0, 1_000));
        let reference = p(3_000.0);
        let agree = tracker.exchanges_agree(Asset::Eth, reference).expect("both exchanges have data");
        assert!(!agree, "exchanges are on opposite sides of reference, should disagree");
    }

    #[test]
    fn exchanges_agree_returns_none_when_binance_missing() {
        let mut tracker = ExchangePriceTracker::new();
        tracker.update(&okx_tick(Asset::Sol, 150.0, 1_000));
        let result = tracker.exchanges_agree(Asset::Sol, p(145.0));
        assert!(result.is_none(), "should return None when binance data is missing");
    }

    #[test]
    fn exchanges_agree_returns_none_when_okx_missing() {
        let mut tracker = ExchangePriceTracker::new();
        tracker.update(&binance_tick(Asset::Sol, 150.0, 1_000));
        let result = tracker.exchanges_agree(Asset::Sol, p(145.0));
        assert!(result.is_none(), "should return None when okx data is missing");
    }

    #[test]
    fn exchanges_agree_returns_none_when_both_missing() {
        let tracker = ExchangePriceTracker::new();
        let result = tracker.exchanges_agree(Asset::Xrp, p(0.5));
        assert!(result.is_none(), "should return None when no data for any exchange");
    }
}
