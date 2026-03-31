# Complete Set Arbitrage Executor — Design Spec

## Goal

Separate binary that replicates the Idolized-Scallops (0xe1d6) trading pattern exactly: buy BOTH Up and Down on the same market when combined cost < $1, guaranteeing profit regardless of outcome. Three execution phases per window: early probe, main sweep, penny picks.

## Reference Trader Stats (186 trades analyzed)

- **ROI**: 273.8% on $7,192 deployed
- **Timing**: 0-49s (10%), 637-840s (57%), 840-900s (27%), 60-600s (0%)
- **Order size**: Median $10.74, spray many small FOK orders
- **Both-sides**: 4 of 7 markets had guaranteed arb (combined < $1)
- **Execution**: 0.59 trades/second, sub-2s gaps, all FOK
- **Assets**: ETH > BTC > XRP > SOL on 15m windows

## Architecture

Separate binary (`polymarket-arb`) sharing crates with directional bot.

```
polymarket-arb binary
  │
  ├─ Market Scanner (Gamma API, reuse pm-market)
  │   → Discovers active 15m windows
  │   → Maps token IDs per (asset, timeframe)
  │
  ├─ Price Feeds (reuse pm-oracle WS)
  │   → Binance + OKX spot prices
  │   → Polymarket contract bid/ask via PM WS
  │
  ├─ Arb Engine (NEW)
  │   → Per-window state machine with 3 phases
  │   → Tracks inventory (Up shares, Down shares, cost basis)
  │   → Decides when to fire FOK orders
  │
  ├─ Order Executor (reuse pm-live CLOB client)
  │   → FOK orders only (no GTC needed)
  │   → Async fire-and-forget, non-blocking
  │
  └─ Oracle Resolution (reuse pm-live polling)
      → Polls CLOB API for market resolution
      → Tracks P&L per window
```

## The Arb Engine — Per-Window State Machine

### State per active window

```rust
struct WindowArb {
    asset: Asset,
    timeframe: Timeframe,
    window_id: WindowId,
    condition_id: String,
    token_id_up: String,
    token_id_down: String,
    window_open_ms: u64,
    window_close_ms: u64,

    // Inventory
    up_shares: f64,
    down_shares: f64,
    up_cost: f64,
    down_cost: f64,

    // Phase tracking
    phase: ArbPhase,
    last_order_ms: u64,
}

enum ArbPhase {
    EarlyProbe,   // 0-49s
    Idle,         // 50-636s (do nothing)
    MainSweep,    // 637-840s
    PennyPicks,   // 841-900s
    Closed,       // post-900s
}
```

### Phase 1: Early Probe (0-49 seconds)

**What**: Check if both sides are cheap enough for instant arb.

```
every 2 seconds:
  ask_up = PM WS best ask for Up token
  ask_down = PM WS best ask for Down token
  combined = ask_up + ask_down

  if combined < 0.92:  // 8% margin covers fees
    FOK Buy Up at ask_up, $20
    FOK Buy Down at ask_down, $20
    → guaranteed profit = (1.0 - combined) × shares
```

**Config**: `early_max_combined = 0.92`, `early_order_size_usdc = 20.0`

### Phase 2: Idle (50-636 seconds)

Do nothing. Wait for prices to diverge.

### Phase 3: Main Sweep (637-840 seconds)

**What**: The market has picked a direction. One side is expensive ($0.60-0.90), the other is cheap ($0.10-0.40). Buy both to build the arb position.

```
every 1-2 seconds:
  ask_up = PM WS best ask
  ask_down = PM WS best ask

  // Current inventory analysis
  combined_cost = (up_cost + down_cost)
  combined_shares = min(up_shares, down_shares)  // matched pairs
  avg_combined_price = if combined_shares > 0 { combined_cost / combined_shares } else { 1.0 }

  // Buy the cheap side (likely the loser)
  cheap_side = if ask_up < ask_down { Up } else { Down }
  cheap_ask = min(ask_up, ask_down)

  if cheap_ask < 0.45 && total_deployed < max_per_window:
    FOK Buy cheap_side at cheap_ask, $10-25

  // Also buy some of the expensive side if combined is still < 0.95
  expensive_ask = max(ask_up, ask_down)
  if cheap_ask + expensive_ask < 0.95:
    FOK Buy expensive_side at expensive_ask, $10-25
```

**Config**: `sweep_max_cheap_ask = 0.45`, `sweep_max_combined = 0.95`, `sweep_order_size = 15.0`, `sweep_interval_ms = 1500`, `max_per_window_usdc = 200.0`

### Phase 4: Penny Picks (841-900 seconds)

**What**: The losing side is now nearly worthless ($0.03-0.15). Buy cheap lottery tickets.

```
every 1 second:
  losing_ask = min(ask_up, ask_down)

  if losing_ask < 0.15 && losing_ask > 0.01:
    FOK Buy losing_side at losing_ask, $5-10
    // Cost: $5. If it flips: payout = $5/0.10 = $50. 10x return.
    // Probability is low but expected value can be positive.
```

**Config**: `penny_max_ask = 0.15`, `penny_order_size = 5.0`, `penny_interval_ms = 1000`

## Order Execution

**All FOK, no GTC.** Reasons:
- Speed: instant fill or reject, no waiting
- No heartbeat needed: FOK doesn't rest on book
- Simpler: no order tracking, cancel logic, or User WS needed
- Matches trader's pattern: sub-2s gaps, fire-and-forget

```rust
// Fire-and-forget FOK
tokio::spawn(async move {
    let result = place_fok_order(&clob, &token_id, Side::Buy, size_usdc, max_price).await;
    match result {
        Ok(fill) => {
            // Send fill back via channel for inventory update
            let _ = fill_tx.send(ArbFill { side, shares: fill.shares, cost: fill.cost_usdc });
        }
        Err(_) => {} // FOK killed, move on
    }
});
```

## Inventory & P&L Tracking

Per window, track:
```
up_shares: 10.0,  up_cost: $4.50   (avg $0.45/share)
down_shares: 15.0, down_cost: $3.00  (avg $0.20/share)
─────────
matched_pairs: 10 (min of up/down)
guaranteed_payout: $10.00 (10 pairs × $1)
total_cost: $7.50
guaranteed_profit: $2.50 (on matched pairs)
unmatched: 5 Down shares at $0.20 each (directional risk: $1.00)
```

After resolution:
- If Down wins: all 15 Down shares pay $1 each = $15.00. Total P&L = $15.00 - $7.50 = +$7.50
- If Up wins: 10 Up shares pay $1 each = $10.00. Total P&L = $10.00 - $7.50 = +$2.50
- Either way: PROFIT (because combined average < $1)

## Config (arb.toml)

```toml
[arb]
# Assets to trade (15m only for now)
assets = ["btc", "eth", "sol"]
timeframe = "min15"

# Capital limits
balance = 500.0
max_per_window_usdc = 200.0
max_total_exposure_usdc = 400.0

# Phase 1: Early Probe (0-49s)
early_enabled = true
early_max_combined = 0.92
early_order_size_usdc = 20.0
early_interval_ms = 2000

# Phase 2: Main Sweep (637-840s)
sweep_enabled = true
sweep_max_cheap_ask = 0.45
sweep_max_combined = 0.95
sweep_order_size_usdc = 15.0
sweep_interval_ms = 1500

# Phase 3: Penny Picks (841-900s)
penny_enabled = true
penny_max_ask = 0.15
penny_order_size_usdc = 5.0
penny_interval_ms = 1000

# CLOB
min_order_size_usdc = 1.0
max_fok_price = 0.85

[data]
cache_dir = "data"
log_dir = "logs"
```

## Binary Structure

```toml
# Cargo.toml addition
[[bin]]
name = "polymarket-arb"
path = "src/arb_main.rs"
```

### Files to Create

| File | Responsibility |
|------|---------------|
| `src/arb_main.rs` | Binary entry point, CLI args, config loading |
| `src/arb_engine.rs` | Per-window state machine, phase logic, inventory |
| `src/arb_executor.rs` | FOK order dispatch, fill channel, P&L tracking |
| `config/arb.toml` | Arb-specific configuration |
| `run-arb.sh` | Launch script |

### Files Reused (no modification needed)

| Crate | What we reuse |
|-------|--------------|
| `pm-live` | `ClobContext`, `init_clob_client`, `place_fok_order`, `check_market_resolution` |
| `pm-market` | `scan_active_markets`, `PolymarketWs`, `LatestPrices` |
| `pm-oracle` | `BinanceWs`, `OkxWs`, `OracleRouter`, `PriceBuffer` |
| `pm-types` | `Asset`, `Side`, `Price`, `Timeframe`, `WindowId` |

## Main Loop

```
1. Init CLOB client (same auth as directional bot)
2. Start Binance + OKX + PM WebSocket
3. Start Gamma API scanner
4. Every tick:
   a. Update spot prices
   b. For each active window:
      - Determine current phase from elapsed time
      - Execute phase logic (fire FOK orders)
      - Update inventory from fill channel
   c. For completed windows:
      - Poll oracle for resolution
      - Log P&L
5. Loop
```

## Risk Management

- **Per-window cap**: $200 max deployment per window
- **Total exposure cap**: $400 across all open windows
- **Kill switch**: Stop if daily P&L < -$50
- **No overlapping arb on same (asset, timeframe)**: one arb per window
- **FOK rejection is fine**: just try again next cycle (1-2s later)

## Comparison: Our Implementation vs Idolized-Scallops

| Aspect | Them | Us |
|--------|------|-----|
| Timing Phase 1 | 13-49s | 0-49s (close match) |
| Timing Phase 2 | 637-840s | 637-840s (exact) |
| Timing Phase 3 | 841-900s | 841-900s (exact) |
| Idle gap | 50-636s | 50-636s (exact) |
| Order type | FOK | FOK (same) |
| Order spacing | 1-2s | 1-2s (same) |
| Order size | Median $10.74 | $5-25 configurable |
| Combined threshold | ~$0.95 inferred | $0.92-0.95 configurable |
| Assets | BTC/ETH/SOL/XRP | BTC/ETH/SOL configurable |
| Timeframe | 15m primary | 15m (same) |
| Both-sides | 65% of markets | Always attempt both |
| Penny picks | $0.03-$0.15 | < $0.15 configurable |
| Trades/hour | ~1,000 | ~200-500 (conservative start) |

## Testing

1. Unit test: ArbPhase transitions at correct timestamps
2. Unit test: Inventory tracking (up_shares, down_shares, cost)
3. Unit test: Combined cost calculation and arb detection
4. Integration: full window lifecycle (open → 3 phases → resolution → P&L)
5. Paper mode: run arb engine against live data without placing orders
