# Signal Enrichment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add cross-exchange price confirmation and multi-timeframe momentum scoring to improve live win rate from ~55% toward 65%+.

**Architecture:** ExchangePriceTracker stores per-exchange prices before the OracleRouter merges them. Momentum score computed from PriceBuffer lookbacks at 30s/60s/120s/240s. Both feed into MarketState as new fields. Strategies use them as confidence modifiers.

**Tech Stack:** Rust, pm-types, pm-oracle, pm-signal crates

---

## File Structure

| File | Responsibility | Action |
|------|---------------|--------|
| `crates/pm-oracle/src/exchange_tracker.rs` | Per-exchange price storage | Create |
| `crates/pm-oracle/src/price_buffer.rs` | Add momentum score function | Modify |
| `crates/pm-oracle/src/lib.rs` | Export new module | Modify |
| `crates/pm-types/src/strategy.rs` | Add 3 fields to MarketState | Modify |
| `src/paper.rs` | Wire tracker + momentum into pipeline | Modify |
| `crates/pm-signal/src/momentum.rs` | Use new signals in confidence | Modify |
| `crates/pm-signal/src/early.rs` | Use new signals in confidence | Modify |
| `crates/pm-signal/src/late_sniper.rs` | Use new signals in confidence | Modify |

---

### Task 1: Create ExchangePriceTracker

**Files:**
- Create: `crates/pm-oracle/src/exchange_tracker.rs`

- [ ] **Step 1: Create the tracker module**

```rust
//! Tracks the latest spot price per (asset, exchange) for cross-exchange confirmation.

use pm_types::{Asset, Price};
use pm_types::asset::ExchangeSource;
use pm_types::market::Tick;

/// Number of exchange sources (Binance, OKX).
const EXCHANGE_COUNT: usize = 2;

/// Stores the most recent price per (asset, exchange) pair.
///
/// Updated from raw ticks BEFORE the OracleRouter merges them,
/// preserving per-exchange price information for cross-exchange
/// confirmation signals.
pub struct ExchangePriceTracker {
    /// [Asset::COUNT][EXCHANGE_COUNT] = 4 assets × 2 exchanges.
    /// Each slot: Option<(price, timestamp_ms)>.
    prices: [[Option<(Price, u64)>; EXCHANGE_COUNT]; Asset::COUNT],
}

impl ExchangePriceTracker {
    /// Create an empty tracker.
    #[must_use]
    pub fn new() -> Self {
        Self {
            prices: [[None; EXCHANGE_COUNT]; Asset::COUNT],
        }
    }

    /// Update with a new tick (call before OracleRouter).
    pub fn update(&mut self, tick: &Tick) {
        let asset_idx = tick.asset.index();
        let exchange_idx = match tick.source {
            ExchangeSource::Binance => 0,
            ExchangeSource::Okx => 1,
        };
        self.prices[asset_idx][exchange_idx] = Some((tick.price, tick.timestamp_ms));
    }

    /// Get the latest price from a specific exchange.
    #[must_use]
    pub fn get(&self, asset: Asset, source: ExchangeSource) -> Option<(Price, u64)> {
        let exchange_idx = match source {
            ExchangeSource::Binance => 0,
            ExchangeSource::Okx => 1,
        };
        self.prices[asset.index()][exchange_idx]
    }

    /// Get Binance price for an asset.
    #[must_use]
    pub fn binance_price(&self, asset: Asset) -> Option<Price> {
        self.prices[asset.index()][0].map(|(p, _)| p)
    }

    /// Get OKX price for an asset.
    #[must_use]
    pub fn okx_price(&self, asset: Asset) -> Option<Price> {
        self.prices[asset.index()][1].map(|(p, _)| p)
    }

    /// Check if both exchanges agree on the direction relative to a reference price.
    ///
    /// Returns `Some(true)` if both are above or both below reference.
    /// Returns `Some(false)` if they disagree.
    /// Returns `None` if either exchange has no data.
    #[must_use]
    pub fn exchanges_agree(&self, asset: Asset, reference: Price) -> Option<bool> {
        let binance = self.binance_price(asset)?;
        let okx = self.okx_price(asset)?;
        let ref_val = reference.as_f64();
        let bin_up = binance.as_f64() >= ref_val;
        let okx_up = okx.as_f64() >= ref_val;
        Some(bin_up == okx_up)
    }
}

impl Default for ExchangePriceTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pm_types::Price;

    fn make_tick(asset: Asset, price: f64, source: ExchangeSource) -> Tick {
        Tick {
            asset,
            price: Price::new(price).unwrap(),
            timestamp_ms: 1000,
            source,
        }
    }

    #[test]
    fn update_and_get() {
        let mut tracker = ExchangePriceTracker::new();
        let tick = make_tick(Asset::Btc, 67000.0, ExchangeSource::Binance);
        tracker.update(&tick);

        assert!(tracker.binance_price(Asset::Btc).is_some());
        assert!(tracker.okx_price(Asset::Btc).is_none());
        assert!((tracker.binance_price(Asset::Btc).unwrap().as_f64() - 67000.0).abs() < 0.01);
    }

    #[test]
    fn exchanges_agree_both_up() {
        let mut tracker = ExchangePriceTracker::new();
        tracker.update(&make_tick(Asset::Btc, 67100.0, ExchangeSource::Binance));
        tracker.update(&make_tick(Asset::Btc, 67050.0, ExchangeSource::Okx));

        let reference = Price::new(67000.0).unwrap();
        assert_eq!(tracker.exchanges_agree(Asset::Btc, reference), Some(true));
    }

    #[test]
    fn exchanges_disagree() {
        let mut tracker = ExchangePriceTracker::new();
        tracker.update(&make_tick(Asset::Btc, 67100.0, ExchangeSource::Binance));
        tracker.update(&make_tick(Asset::Btc, 66900.0, ExchangeSource::Okx));

        let reference = Price::new(67000.0).unwrap();
        assert_eq!(tracker.exchanges_agree(Asset::Btc, reference), Some(false));
    }

    #[test]
    fn missing_exchange_returns_none() {
        let mut tracker = ExchangePriceTracker::new();
        tracker.update(&make_tick(Asset::Btc, 67100.0, ExchangeSource::Binance));

        let reference = Price::new(67000.0).unwrap();
        assert_eq!(tracker.exchanges_agree(Asset::Btc, reference), None);
    }
}
```

- [ ] **Step 2: Export from pm-oracle lib.rs**

Add to `crates/pm-oracle/src/lib.rs`:
```rust
pub mod exchange_tracker;
pub use exchange_tracker::ExchangePriceTracker;
```

- [ ] **Step 3: Verify build**

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo build -p pm-oracle`

- [ ] **Step 4: Run tests**

Run: `cargo test -p pm-oracle`

- [ ] **Step 5: Commit**

```bash
git add crates/pm-oracle/src/exchange_tracker.rs crates/pm-oracle/src/lib.rs
git commit -m "feat(pm-oracle): add ExchangePriceTracker for cross-exchange confirmation"
```

---

### Task 2: Add Momentum Score Function to PriceBuffer

**Files:**
- Modify: `crates/pm-oracle/src/price_buffer.rs`

- [ ] **Step 1: Add compute_momentum_score function**

Add at the end of the `impl PriceBuffer` block:

```rust
    /// Compute a multi-timeframe momentum score from price history.
    ///
    /// Queries the buffer at 30s, 60s, 120s, 240s lookbacks and computes
    /// weighted slopes. Longer lookbacks weighted higher to filter noise.
    ///
    /// Returns a value in roughly [-0.01, +0.01] range.
    /// Positive = upward momentum, negative = downward.
    #[must_use]
    pub fn momentum_score(&self, asset: Asset, now_ms: u64, current_price: f64) -> f64 {
        const LOOKBACKS_MS: [u64; 4] = [30_000, 60_000, 120_000, 240_000];
        const WEIGHTS: [f64; 4] = [0.15, 0.20, 0.30, 0.35];

        let mut score = 0.0;
        let mut total_weight = 0.0;

        for (&lb, &w) in LOOKBACKS_MS.iter().zip(WEIGHTS.iter()) {
            if let Some(past_price) = self.price_at(asset, now_ms.saturating_sub(lb)) {
                let past = past_price.as_f64();
                if past > 0.0 {
                    let slope = (current_price - past) / past;
                    score += slope * w;
                    total_weight += w;
                }
            }
        }

        if total_weight > 0.0 {
            score / total_weight
        } else {
            0.0
        }
    }
```

- [ ] **Step 2: Add tests**

Add at the end of the test module:

```rust
    #[test]
    fn momentum_score_uptrend() {
        let mut buf = PriceBuffer::new();
        // Seed prices going up over 240 seconds
        for i in 0..250 {
            let price = 67000.0 + (i as f64) * 2.0; // +$2 per second
            buf.push(Asset::Btc, i * 1000, Price::new(price).unwrap());
        }
        let score = buf.momentum_score(Asset::Btc, 249_000, 67498.0);
        assert!(score > 0.0, "uptrend should have positive score: {score}");
    }

    #[test]
    fn momentum_score_downtrend() {
        let mut buf = PriceBuffer::new();
        for i in 0..250 {
            let price = 67000.0 - (i as f64) * 2.0;
            buf.push(Asset::Btc, i * 1000, Price::new(price).unwrap());
        }
        let score = buf.momentum_score(Asset::Btc, 249_000, 66502.0);
        assert!(score < 0.0, "downtrend should have negative score: {score}");
    }

    #[test]
    fn momentum_score_empty_buffer() {
        let buf = PriceBuffer::new();
        let score = buf.momentum_score(Asset::Btc, 100_000, 67000.0);
        assert!((score - 0.0).abs() < f64::EPSILON, "empty buffer should return 0");
    }
```

- [ ] **Step 3: Verify**

Run: `cargo test -p pm-oracle`

- [ ] **Step 4: Commit**

```bash
git add crates/pm-oracle/src/price_buffer.rs
git commit -m "feat(pm-oracle): add multi-timeframe momentum_score to PriceBuffer"
```

---

### Task 3: Add New Fields to MarketState

**Files:**
- Modify: `crates/pm-types/src/strategy.rs`

- [ ] **Step 1: Add 3 new fields to MarketState**

After the `orderbook_imbalance` field (around line 92), add:

```rust
    /// Latest Binance spot price for cross-exchange confirmation.
    pub binance_price: Option<Price>,
    /// Latest OKX spot price for cross-exchange confirmation.
    pub okx_price: Option<Price>,
    /// Multi-timeframe momentum score from weighted 30s/60s/120s/240s slopes.
    /// Positive = upward momentum, negative = downward. Range ~[-0.01, +0.01].
    pub momentum_score: f64,
```

- [ ] **Step 2: Update all MarketState constructions in tests**

Search for `MarketState {` in the test modules and add the 3 new fields with defaults:
```rust
binance_price: None,
okx_price: None,
momentum_score: 0.0,
```

This includes tests in:
- `crates/pm-types/src/strategy.rs` (make_state helper)
- `crates/pm-signal/src/momentum.rs` (test helpers)
- `crates/pm-signal/src/early.rs` (test helpers)
- `crates/pm-signal/src/late_sniper.rs` (test helpers)
- `crates/pm-signal/src/instance.rs` (make_state helper)

- [ ] **Step 3: Verify build + tests**

Run: `cargo test`

- [ ] **Step 4: Commit**

```bash
git add crates/pm-types/src/strategy.rs crates/pm-signal/src/
git commit -m "feat(pm-types): add binance_price, okx_price, momentum_score to MarketState"
```

---

### Task 4: Wire into Paper Loop

**Files:**
- Modify: `src/paper.rs`

- [ ] **Step 1: Add ExchangePriceTracker to run_paper**

Near the other tracker initializations (around line 760), add:

```rust
let mut exchange_tracker = pm_oracle::ExchangePriceTracker::new();
```

- [ ] **Step 2: Update tick before OracleRouter**

In the main event loop where ticks are received (around line 1204), before `oracle_router.process(tick)`, add:

```rust
exchange_tracker.update(&tick);
```

- [ ] **Step 3: Pass exchange_tracker and price_buffer to process_tick**

Add `exchange_tracker: &ExchangePriceTracker` and `price_buffer: &PriceBuffer` to the `process_tick` function signature. Update the call site to pass them.

- [ ] **Step 4: Populate new MarketState fields in build_market_state**

In the `build_market_state` function (or where MarketState is constructed), add:

```rust
binance_price: exchange_tracker.binance_price(tick.asset),
okx_price: exchange_tracker.okx_price(tick.asset),
momentum_score: price_buffer.momentum_score(tick.asset, tick.timestamp_ms, tick.price.as_f64()),
```

- [ ] **Step 5: Verify build**

Run: `cargo build --release`

- [ ] **Step 6: Commit**

```bash
git add src/paper.rs
git commit -m "feat: wire ExchangePriceTracker and momentum_score into paper loop"
```

---

### Task 5: Use New Signals in Strategies

**Files:**
- Modify: `crates/pm-signal/src/momentum.rs`
- Modify: `crates/pm-signal/src/early.rs`
- Modify: `crates/pm-signal/src/late_sniper.rs`

- [ ] **Step 1: Add signal enrichment to MomentumConfirmation**

In `crates/pm-signal/src/momentum.rs`, after the existing orderbook imbalance boost block, add:

```rust
        // Cross-exchange confirmation: penalize if exchanges disagree.
        if let (Some(bp), Some(op)) = (state.binance_price, state.okx_price) {
            let bin_up = bp.as_f64() > state.window_open_price.as_f64();
            let okx_up = op.as_f64() > state.window_open_price.as_f64();
            if bin_up != okx_up {
                confidence *= 0.5;
            }
        }

        // Multi-timeframe momentum: boost if aligned, penalize if opposing.
        let momentum_aligned = match state.spot_direction {
            Side::Up => state.momentum_score > 0.0005,
            Side::Down => state.momentum_score < -0.0005,
        };
        if momentum_aligned {
            confidence = (confidence + 0.15).min(1.0);
        } else if state.momentum_score.abs() > 0.0005 {
            confidence = (confidence - 0.15).max(0.0);
        }
```

- [ ] **Step 2: Add same enrichment to EarlyDirectional**

Copy the same cross-exchange + momentum block to `crates/pm-signal/src/early.rs` after the existing imbalance boost.

- [ ] **Step 3: Add same enrichment to LateWindowSniper**

Copy the same cross-exchange + momentum block to `crates/pm-signal/src/late_sniper.rs` after the existing imbalance boost.

- [ ] **Step 4: Verify build + tests**

Run: `cargo test`

- [ ] **Step 5: Commit**

```bash
git add crates/pm-signal/src/
git commit -m "feat(pm-signal): use cross-exchange and momentum signals in confidence"
```

---

### Task 6: Full Build Verification

- [ ] **Step 1: Release build**

Run: `cargo build --release`

- [ ] **Step 2: All tests**

Run: `cargo test`

- [ ] **Step 3: Verify signal logging shows new fields**

Check that the SIGNAL FIRED log will include the new data by reviewing the log statement in `instance.rs`. If not already logging momentum_score, add it to the info! macro.
