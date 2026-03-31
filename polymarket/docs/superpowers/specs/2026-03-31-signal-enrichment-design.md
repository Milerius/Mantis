# Signal Enrichment: Cross-Exchange + Multi-Timeframe Momentum

## Goal

Add two new signal dimensions to improve live win rate from ~55% toward 65%+:
1. Cross-exchange price confirmation (Binance vs OKX)
2. Multi-timeframe momentum score (30s/60s/120s/240s weighted slopes)

Both are used as confidence modifiers — they don't change entry thresholds, they adjust how much conviction the strategy has.

## Component 1: ExchangePriceTracker

### What it does
Stores the latest spot price per (asset, exchange) pair. Updated from raw ticks before the OracleRouter merges them.

### Data structure
```rust
pub struct ExchangePriceTracker {
    /// [Asset::COUNT][ExchangeSource variants] = 4 assets × 2 exchanges
    prices: [[Option<(Price, u64)>; 2]; 4],
}
```

Methods:
- `update(&mut self, tick: &Tick)` — store price + timestamp for (asset, source)
- `get(&self, asset: Asset, source: ExchangeSource) -> Option<(Price, u64)>`
- `cross_exchange_agrees(&self, asset: Asset, reference_price: Price) -> Option<bool>` — true if both exchanges show price on the same side of reference (e.g., both above window open = both agree Up)

### New MarketState fields
```rust
pub binance_price: Option<Price>,
pub okx_price: Option<Price>,
```

### Strategy usage
In confidence calculation, after existing formula:
```rust
if let (Some(bp), Some(op)) = (state.binance_price, state.okx_price) {
    let bin_up = bp.as_f64() > state.window_open_price.as_f64();
    let okx_up = op.as_f64() > state.window_open_price.as_f64();
    if bin_up != okx_up {
        confidence *= 0.5; // Exchanges disagree — halve conviction
    }
}
```

## Component 2: Multi-Timeframe Momentum Score

### What it does
Computes a composite momentum score from price slopes at 4 lookback periods (30s, 60s, 120s, 240s). Longer lookbacks weighted higher to filter micro-bounces.

### Computation
```rust
pub fn compute_momentum_score(
    buffer: &PriceBuffer,
    asset: Asset,
    now_ms: u64,
    current_price: f64,
) -> f64 {
    let lookbacks = [30_000u64, 60_000, 120_000, 240_000]; // ms
    let weights = [0.15, 0.20, 0.30, 0.35];

    let mut score = 0.0;
    let mut total_weight = 0.0;

    for (&lb, &w) in lookbacks.iter().zip(weights.iter()) {
        if let Some(past_price) = buffer.price_at(asset, now_ms.saturating_sub(lb)) {
            let slope = (current_price - past_price.as_f64()) / past_price.as_f64();
            score += slope * w;
            total_weight += w;
        }
    }

    if total_weight > 0.0 { score / total_weight } else { 0.0 }
}
```

Returns a value in roughly [-0.01, +0.01] range (percentage move). Positive = Up momentum, negative = Down.

### New MarketState field
```rust
pub momentum_score: f64,
```

### Strategy usage
In confidence calculation:
```rust
let momentum_aligned = match state.spot_direction {
    Side::Up => state.momentum_score > 0.0005,   // >0.05% composite momentum
    Side::Down => state.momentum_score < -0.0005,
};
if momentum_aligned {
    confidence = (confidence + 0.15).min(1.0); // All timeframes agree
} else if state.momentum_score.abs() > 0.0005 {
    confidence = (confidence - 0.15).max(0.0); // Timeframes disagree
}
// If momentum is near zero (< 0.05%), no adjustment
```

## Pipeline Changes

### paper.rs — tick processing
```
Raw tick arrives
  ↓
exchange_tracker.update(&tick)          ← NEW (before router)
  ↓
oracle_router.process(tick)             ← existing
  ↓
price_buffer.push(asset, ts, price)     ← existing
  ↓
process_tick(... exchange_tracker, price_buffer ...) ← pass new refs
  ↓
build_market_state(... exchange_tracker, price_buffer ...) ← populate new fields
```

### build_market_state additions
```rust
// Cross-exchange prices
binance_price: exchange_tracker.get(tick.asset, ExchangeSource::Binance).map(|(p, _)| p),
okx_price: exchange_tracker.get(tick.asset, ExchangeSource::Okx).map(|(p, _)| p),

// Momentum score from price buffer
momentum_score: compute_momentum_score(price_buffer, tick.asset, tick.timestamp_ms, tick.price.as_f64()),
```

## Files to Create/Modify

### Create
- `crates/pm-oracle/src/exchange_tracker.rs` — ExchangePriceTracker struct

### Modify
- `crates/pm-oracle/src/lib.rs` — export new module
- `crates/pm-oracle/src/price_buffer.rs` — add `compute_momentum_score` function
- `crates/pm-types/src/strategy.rs` — add 3 fields to MarketState
- `src/paper.rs` — wire tracker + momentum into pipeline
- `crates/pm-signal/src/momentum.rs` — use new signals in confidence
- `crates/pm-signal/src/early.rs` — use new signals in confidence
- `crates/pm-signal/src/late_sniper.rs` — use new signals in confidence

## Testing
- Unit test: ExchangePriceTracker update/query
- Unit test: compute_momentum_score with known buffer data
- Unit test: cross-exchange disagreement halves confidence
- Unit test: momentum alignment boosts confidence
- Integration: build + all existing tests pass
