# Live Execution Module — Taker Mode (Phase 1)

## Goal

Add real order execution to the Polymarket trading bot using the official `polymarket-client-sdk` Rust crate. When a strategy signal fires on a `mode = "live"` instance, place a real FOK market order on the CLOB instead of simulating a fill. Paper and live instances run side-by-side through the same `StrategyInstance` trait.

## Architecture

### LiveStrategyInstance (wrapper pattern)

```rust
pub struct LiveStrategyInstance {
    /// Inner paper instance — evaluates signals, tracks paper P&L for comparison
    paper: ConcreteStrategyInstance,

    /// Authenticated CLOB client (shared across all live instances via Arc)
    clob: Arc<AuthenticatedClobClient>,

    /// Signer for order signing
    signer: Arc<LocalSigner>,

    /// Real balance and stats (separate from paper tracking)
    real_balance: f64,
    real_pnl: f64,
    real_stats: InstanceStats,

    /// Per-window position tracking for real positions
    real_positions: Vec<RealPosition>,

    /// Window slot dedup (same as paper — one position per asset/tf per window)
    window_slots: [Option<WindowId>; 32],

    /// Token ID mapping from market scanner
    token_map: Arc<Mutex<HashMap<(Asset, Timeframe), TokenPair>>>,
}

struct TokenPair {
    up: String,
    down: String,
}

struct RealPosition {
    window_id: WindowId,
    asset: Asset,
    side: Side,
    fill_price: f64,
    size_usdc: f64,
    shares: f64,
    order_id: String,
    slot: usize,
}
```

### on_tick Flow

```
fn on_tick(&mut self, state: &MarketState) -> Option<FillEvent> {
    // 1. Slot check (same as paper)
    let slot = asset_idx * TF_COUNT + tf_idx;
    if self.window_slots[slot] == Some(state.window_id) { return None; }

    // 2. Kill switch
    if self.real_pnl < -self.paper.max_daily_loss { return None; }

    // 3. Evaluate signal via paper instance (pure, no side effects)
    //    We call the strategy evaluator directly, not paper.on_tick()
    let decision = self.paper.strategy.evaluate(state)?;

    // 4. Exposure check on real balance
    let exposure: f64 = self.real_positions.iter().map(|p| p.size_usdc).sum();
    if exposure >= self.paper.max_exposure_usdc { return None; }

    // 5. Kelly sizing on real balance
    let size = (self.paper.kelly_fraction * decision.confidence * self.real_balance)
        .min(self.paper.max_position_usdc)
        .min(self.real_balance * 0.05);
    if size <= 0.0 { return None; }

    // 6. Resolve token ID from scanner map
    let token_id = self.get_token_id(state.asset, state.timeframe, decision.side)?;

    // 7. Place FOK market order via CLOB
    let fill_result = self.place_market_order(&token_id, size, decision.side).await;

    match fill_result {
        Ok(fill) => {
            self.real_balance -= fill.cost_usdc;
            self.real_positions.push(RealPosition { ... });
            self.window_slots[slot] = Some(state.window_id);

            // Also track in paper for comparison
            let _ = self.paper.on_tick(state);

            Some(FillEvent {
                label: self.paper.label,
                fill_price: fill.avg_price,
                size_usdc: fill.cost_usdc,
                ...
            })
        }
        Err(e) => {
            warn!(instance = %self.label(), error = %e, "live order failed");
            None
        }
    }
}
```

### on_window_close Flow

Polymarket binary contracts auto-settle. When a window resolves:

```
fn on_window_close(&mut self, window_id: WindowId, outcome: Side, ts: u64) -> Vec<TradeRecord> {
    // Close real positions for this window
    let mut trades = Vec::new();
    let mut i = 0;
    while i < self.real_positions.len() {
        if self.real_positions[i].window_id != window_id {
            i += 1; continue;
        }
        let pos = self.real_positions.swap_remove(i);
        let won = pos.side == outcome;
        let pnl = if won {
            pos.shares * 1.0 - pos.size_usdc  // shares × $1 payout - cost
        } else {
            -pos.size_usdc
        };

        self.real_balance += pos.size_usdc + pnl;
        self.real_pnl += pnl;
        self.real_stats.record(pnl);
        self.window_slots[pos.slot] = None;

        trades.push(TradeRecord { ... });
    }

    // Also resolve paper positions for comparison
    let _ = self.paper.on_window_close(window_id, outcome, ts);

    trades
}
```

### CLOB Client Setup

```rust
use polymarket_client_sdk::clob::{Client, Config};
use polymarket_client_sdk::clob::types::{Amount, OrderType, Side as ClobSide};
use alloy::signers::local::LocalSigner;

async fn create_clob_client() -> Result<(Arc<Client<Authenticated>>, Arc<LocalSigner>)> {
    let private_key = std::env::var("POLYMARKET_PRIVATE_KEY")
        .context("POLYMARKET_PRIVATE_KEY env var required for live trading")?;

    let signer = LocalSigner::from_str(&private_key)?
        .with_chain_id(Some(137));  // Polygon

    let client = Client::new("https://clob.polymarket.com", Config::default())?
        .authentication_builder(&signer)
        .signature_type(SignatureType::GnosisSafe)  // proxy wallet
        .authenticate()
        .await?;

    Ok((Arc::new(client), Arc::new(signer)))
}
```

### Market Order Placement

```rust
async fn place_market_order(
    &self,
    token_id: &str,
    size_usdc: f64,
    side: Side,
) -> Result<LiveFill> {
    let amount = Amount::usdc(Decimal::from_f64(size_usdc).unwrap())?;
    let clob_side = match side {
        Side::Up => ClobSide::Buy,
        Side::Down => ClobSide::Buy,  // always buying the token
    };

    let order = self.clob
        .market_order()
        .token_id(token_id)
        .amount(amount)
        .side(clob_side)
        .order_type(OrderType::FOK)
        .build()
        .await?;

    let signed = self.clob.sign(&self.signer, order).await?;
    let response = self.clob.post_order(signed).await?;

    Ok(LiveFill {
        order_id: response.order_id,
        avg_price: response.avg_price,
        cost_usdc: size_usdc,
        shares: size_usdc / response.avg_price,
    })
}
```

### Config Changes

Add `mode` field to each `StrategyConfig` variant:

```rust
// In config.rs
#[serde(default = "default_mode")]
pub mode: String,  // "paper" or "live"

fn default_mode() -> String { "paper".to_string() }
```

TOML:

```toml
[[bot.strategies]]
type = "momentum_confirmation"
label = "MC-tight"
mode = "live"
balance = 500.0
# ...
```

### Builder Changes

```rust
pub async fn build_instances_from_config(
    strategies: &[StrategyConfig],
    clob_client: Option<(Arc<Client>, Arc<LocalSigner>)>,
    token_map: Arc<Mutex<HashMap<(Asset, Timeframe), TokenPair>>>,
) -> Vec<Box<dyn StrategyInstance>> {
    strategies.iter().map(|s| {
        let mode = s.mode();  // "paper" or "live"
        let paper = build_paper_instance(s);

        if mode == "live" {
            let (client, signer) = clob_client.clone()
                .expect("CLOB client required for live mode");
            Box::new(LiveStrategyInstance::new(
                paper, client, signer, token_map.clone(),
            )) as Box<dyn StrategyInstance>
        } else {
            Box::new(paper) as Box<dyn StrategyInstance>
        }
    }).collect()
}
```

### Main Loop — Zero Changes

```rust
// This code doesn't change at all
for instance in &mut instances {
    if let Some(fill) = instance.on_tick(&state) {
        info!(instance = %fill.label, ...);
    }
}
```

### Logging

Live fills get a distinct log prefix:

```
LIVE FILL  instance=MC-tight asset=ETH side=Up price=$0.52 size=$25 order_id=0x1a2b... balance=$475
PAPER FILL instance=MC-loose asset=ETH side=Up price=$0.52 size=$6.25 balance=$118.75
```

### Token Map Population

The market scanner already discovers `token_id_up` and `token_id_down` for each market. We pass this mapping to `LiveStrategyInstance` via a shared `Arc<Mutex<HashMap>>` (same pattern as the existing `SharedTokenAssetMap`).

### Error Handling

| Error | Action |
|-------|--------|
| Order rejected (insufficient balance) | Log warning, skip, don't retry |
| Order rejected (market closed) | Log warning, skip |
| Network timeout | Retry once with 1s backoff, then skip |
| 429 rate limit | Wait + retry (SDK handles this) |
| Invalid token ID | Log error, skip (scanner stale?) |
| Signing failure | Log error, activate kill switch |

### Safety Guards

1. **Max position size capped** — `max_position_usdc` from config, never exceeded
2. **Daily loss kill switch** — same as paper, stops all live trading
3. **Exposure limit** — total open positions capped at `max_exposure_usdc`
4. **Startup balance check** — query on-chain USDC balance at startup, refuse to trade if below minimum
5. **Paper comparison** — paper instance runs in parallel, log divergence between paper and live fills

## New Crate

```
crates/pm-live/
├── Cargo.toml     (depends on polymarket-client-sdk, pm-types, pm-signal)
├── src/
│   ├── lib.rs
│   ├── instance.rs    — LiveStrategyInstance
│   └── clob.rs        — CLOB client wrapper, order helpers
```

## Files Affected

| File | Change |
|------|--------|
| `Cargo.toml` (workspace) | Add `pm-live` crate |
| `crates/pm-live/` | New crate |
| `crates/pm-types/src/config.rs` | Add `mode` field to StrategyConfig |
| `crates/pm-signal/src/builder.rs` | Update builder to accept CLOB client |
| `crates/pm-signal/src/instance.rs` | Make strategy evaluator accessible for live wrapper |
| `src/paper.rs` | Initialize CLOB client if any strategy is "live", pass to builder |
| `config/production.toml` | Add `mode` field examples |

## Not In Scope

- Maker mode (limit orders, cancel/replace) — Phase 2
- Cross-venue arbitrage (Kalshi) — separate module
- On-chain USDC deposit/withdrawal — manual via Polymarket UI
- Gas management (POL for Polygon) — manual top-up
- Multi-wallet support — single signer for now
