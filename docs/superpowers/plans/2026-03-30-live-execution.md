# Live Execution Module Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add real CLOB order execution so strategy instances with `mode = "live"` place real FOK market orders on Polymarket while paper instances continue simulating.

**Architecture:** New `pm-live` crate with `LiveStrategyInstance` that wraps `ConcreteStrategyInstance`. Uses the official `polymarket-client-sdk` for authentication, order signing, and placement. The main loop doesn't change — it calls `instance.on_tick()` regardless of paper vs live.

**Tech Stack:** Rust, `polymarket-client-sdk` (v0.4), `alloy` (signers), `tokio` (async), `rust_decimal`.

---

### Task 1: Add `mode` field to StrategyConfig

**Files:**
- Modify: `crates/pm-types/src/config.rs`

- [ ] **Step 1: Add `mode` field to all 6 StrategyConfig variants**

In `crates/pm-types/src/config.rs`, add to each variant (EarlyDirectional, MomentumConfirmation, CompleteSetArb, HedgeLock, LateWindowSniper, MeanReversion):

```rust
#[serde(default = "default_strategy_mode")]
mode: String,
```

Add the default function:

```rust
fn default_strategy_mode() -> String { String::from("paper") }
```

Add a helper method on `StrategyConfig`:

```rust
impl StrategyConfig {
    /// Get the mode for this strategy ("paper" or "live").
    pub fn mode(&self) -> &str {
        match self {
            Self::EarlyDirectional { mode, .. }
            | Self::MomentumConfirmation { mode, .. }
            | Self::CompleteSetArb { mode, .. }
            | Self::HedgeLock { mode, .. }
            | Self::LateWindowSniper { mode, .. }
            | Self::MeanReversion { mode, .. } => mode,
        }
    }
}
```

- [ ] **Step 2: Update `default_strategies()` to include mode**

Add `mode: String::new()` (defaults to "paper" via serde) to each variant in `default_strategies()`.

- [ ] **Step 3: Fix all test StrategyConfig constructions**

Search for `StrategyConfig::EarlyDirectional {` etc. in test files and add `mode: String::new()` where needed.

- [ ] **Step 4: Run tests**

Run: `cargo test --workspace`
Expected: All tests pass. Existing TOML without `mode` deserializes with default "paper".

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(config): add per-strategy mode field (paper/live)"
```

---

### Task 2: Expose strategy evaluator from ConcreteStrategyInstance

**Files:**
- Modify: `crates/pm-signal/src/instance.rs`

- [ ] **Step 1: Add public accessor for the inner strategy evaluator**

In `crates/pm-signal/src/instance.rs`, add a method to `ConcreteStrategyInstance`:

```rust
impl ConcreteStrategyInstance {
    // ... existing methods ...

    /// Access the inner strategy evaluator (for LiveStrategyInstance wrapper).
    pub fn evaluate_signal(&self, state: &MarketState) -> Option<EntryDecision> {
        use crate::strategy_trait::Strategy;
        self.strategy.evaluate(state)
    }

    /// Get the max position size config.
    pub fn max_position_usdc(&self) -> f64 { self.max_position_usdc }

    /// Get the max exposure config.
    pub fn max_exposure_usdc(&self) -> f64 { self.max_exposure_usdc }

    /// Get the kelly fraction config.
    pub fn kelly_fraction(&self) -> f64 { self.kelly_fraction }

    /// Get the max daily loss config.
    pub fn max_daily_loss(&self) -> f64 { self.max_daily_loss }

    /// Get the slippage in decimal form.
    pub fn slippage(&self) -> f64 { self.slippage }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p pm-signal`

- [ ] **Step 3: Commit**

```bash
git commit -am "feat(pm-signal): expose strategy evaluator and risk params for live wrapper"
```

---

### Task 3: Create pm-live crate with LiveStrategyInstance

**Files:**
- Create: `crates/pm-live/Cargo.toml`
- Create: `crates/pm-live/src/lib.rs`
- Create: `crates/pm-live/src/clob.rs`
- Create: `crates/pm-live/src/instance.rs`
- Modify: `Cargo.toml` (workspace)

- [ ] **Step 1: Create crate skeleton**

Create `crates/pm-live/Cargo.toml`:

```toml
[package]
name = "pm-live"
version = "0.1.0"
edition = "2024"

[dependencies]
pm-types = { path = "../pm-types", features = ["std"] }
pm-signal = { path = "../pm-signal" }
polymarket-client-sdk = { version = "0.4", features = ["clob"] }
alloy = { version = "0.15", features = ["signers"] }
rust_decimal = "1"
tokio = { version = "1", features = ["rt"] }
tracing = "0.1"
anyhow = "1"
```

Add to workspace `Cargo.toml` members:

```toml
members = [
    # ... existing ...
    "crates/pm-live",
]
```

- [ ] **Step 2: Create CLOB client wrapper**

Create `crates/pm-live/src/clob.rs`:

```rust
//! CLOB client initialization and order placement helpers.

use std::str::FromStr;
use std::sync::Arc;

use alloy::signers::local::LocalSigner;
use anyhow::{Context, Result};
use polymarket_client_sdk::clob::{Client, Config};
use polymarket_client_sdk::clob::types::{Amount, OrderType, Side as ClobSide};
use rust_decimal::Decimal;
use tracing::{info, warn};

/// Authenticated CLOB client + signer pair.
pub struct ClobContext {
    pub client: Arc<Client>,  // The exact generic type depends on SDK's auth state machine
    pub signer: Arc<LocalSigner>,
}

/// Result of a successful market order fill.
#[derive(Debug, Clone)]
pub struct LiveFill {
    pub order_id: String,
    pub avg_price: f64,
    pub cost_usdc: f64,
    pub shares: f64,
}

/// Initialize the CLOB client from POLYMARKET_PRIVATE_KEY env var.
///
/// Returns None if the env var is not set (paper-only mode).
/// Returns Err if the key is set but authentication fails.
pub async fn init_clob_client() -> Result<Option<ClobContext>> {
    let private_key = match std::env::var("POLYMARKET_PRIVATE_KEY") {
        Ok(key) => key,
        Err(_) => {
            info!("POLYMARKET_PRIVATE_KEY not set — live trading disabled");
            return Ok(None);
        }
    };

    let signer = LocalSigner::from_str(&private_key)
        .context("invalid POLYMARKET_PRIVATE_KEY")?
        .with_chain_id(Some(137)); // Polygon

    info!(address = %signer.address(), "CLOB signer initialized");

    let client = Client::new("https://clob.polymarket.com", Config::default())
        .context("failed to create CLOB client")?
        .authentication_builder(&signer)
        .authenticate()
        .await
        .context("CLOB authentication failed")?;

    info!("CLOB client authenticated successfully");

    Ok(Some(ClobContext {
        client: Arc::new(client),
        signer: Arc::new(signer),
    }))
}

/// Place a Fill-or-Kill market order.
pub async fn place_fok_order(
    ctx: &ClobContext,
    token_id: &str,
    size_usdc: f64,
) -> Result<LiveFill> {
    let amount = Amount::usdc(
        Decimal::from_f64_retain(size_usdc)
            .context("invalid size_usdc for Decimal conversion")?
    )?;

    let order = ctx.client
        .market_order()
        .token_id(token_id)
        .amount(amount)
        .side(ClobSide::Buy)
        .order_type(OrderType::FOK)
        .build()
        .await
        .context("failed to build market order")?;

    let signed = ctx.client
        .sign(&ctx.signer, order)
        .await
        .context("failed to sign order")?;

    let response = ctx.client
        .post_order(signed)
        .await
        .context("failed to post order")?;

    Ok(LiveFill {
        order_id: format!("{:?}", response),  // SDK response format TBD
        avg_price: 0.0,   // Extract from response
        cost_usdc: size_usdc,
        shares: 0.0,       // Extract from response
    })
}
```

**Note:** The exact SDK types for `Client<Authenticated>` and response fields depend on the SDK version. The implementer should `cargo doc --open` the SDK to check exact types and adjust.

- [ ] **Step 3: Create LiveStrategyInstance**

Create `crates/pm-live/src/instance.rs`:

```rust
//! Live execution wrapper around ConcreteStrategyInstance.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use pm_signal::ConcreteStrategyInstance;
use pm_types::{
    Asset, FillEvent, InstanceStats, MarketState, OpenPosition, Pnl, Side,
    StrategyInstance, Timeframe, TradeRecord, WindowId,
    trade::OrderReason,
};
use tracing::{info, warn};

use crate::clob::{ClobContext, LiveFill, place_fok_order};

/// Maximum number of (asset, timeframe) slots.
const MAX_SLOTS: usize = 32;

/// Token IDs for a market's Up and Down outcomes.
#[derive(Clone, Debug)]
pub struct TokenPair {
    pub up: String,
    pub down: String,
}

/// Shared token map: (Asset, Timeframe) → TokenPair.
pub type SharedTokenMap = Arc<Mutex<HashMap<(Asset, Timeframe), TokenPair>>>;

/// A real-money position.
struct RealPosition {
    window_id: WindowId,
    asset: Asset,
    timeframe: Timeframe,
    side: Side,
    fill_price: f64,
    size_usdc: f64,
    shares: f64,
    order_id: String,
    slot: usize,
}

/// Wraps a ConcreteStrategyInstance with real CLOB execution.
pub struct LiveStrategyInstance {
    /// Inner paper instance for signal evaluation + paper P&L comparison.
    paper: ConcreteStrategyInstance,

    /// CLOB client context (shared).
    clob: Arc<ClobContext>,

    /// Token ID mapping from scanner.
    token_map: SharedTokenMap,

    /// Real balance tracking.
    real_balance: f64,
    real_pnl: f64,
    real_stats: InstanceStats,

    /// Real open positions.
    real_positions: Vec<RealPosition>,

    /// Window slot dedup.
    window_slots: [Option<WindowId>; MAX_SLOTS],

    /// Tokio runtime handle for async order placement.
    rt_handle: tokio::runtime::Handle,
}

impl LiveStrategyInstance {
    pub fn new(
        paper: ConcreteStrategyInstance,
        clob: Arc<ClobContext>,
        token_map: SharedTokenMap,
    ) -> Self {
        let balance = paper.balance();
        Self {
            paper,
            clob,
            token_map,
            real_balance: balance,
            real_pnl: 0.0,
            real_stats: InstanceStats::default(),
            real_positions: Vec::new(),
            window_slots: [None; MAX_SLOTS],
            rt_handle: tokio::runtime::Handle::current(),
        }
    }

    /// Look up token ID for (asset, timeframe, side).
    fn get_token_id(&self, asset: Asset, timeframe: Timeframe, side: Side) -> Option<String> {
        let map = self.token_map.lock().ok()?;
        let pair = map.get(&(asset, timeframe))?;
        Some(match side {
            Side::Up => pair.up.clone(),
            Side::Down => pair.down.clone(),
        })
    }
}

impl StrategyInstance for LiveStrategyInstance {
    fn label(&self) -> &str {
        self.paper.label()
    }

    fn on_tick(&mut self, state: &MarketState) -> Option<FillEvent> {
        // 1. Slot check
        let slot = state.asset.index() * Timeframe::COUNT + state.timeframe.index();
        if let Some(wid) = self.window_slots[slot] {
            if wid == state.window_id {
                return None;
            }
        }

        // 2. Kill switch
        if self.real_pnl < -self.paper.max_daily_loss() {
            return None;
        }

        // 3. Evaluate signal (pure, no side effects)
        let decision = self.paper.evaluate_signal(state)?;

        // 4. Exposure check
        let exposure: f64 = self.real_positions.iter().map(|p| p.size_usdc).sum();
        if exposure >= self.paper.max_exposure_usdc() {
            return None;
        }

        // 5. Kelly sizing on real balance
        let size = (self.paper.kelly_fraction() * decision.confidence * self.real_balance)
            .min(self.paper.max_position_usdc())
            .min(self.real_balance * 0.05);
        if size <= 0.0 {
            return None;
        }

        // 6. Resolve token ID
        let token_id = self.get_token_id(state.asset, state.timeframe, decision.side)?;

        // 7. Place FOK market order (block on async)
        let clob = self.clob.clone();
        let fill_result = self.rt_handle.block_on(async {
            place_fok_order(&clob, &token_id, size).await
        });

        match fill_result {
            Ok(fill) => {
                self.real_balance -= fill.cost_usdc;
                self.real_positions.push(RealPosition {
                    window_id: state.window_id,
                    asset: state.asset,
                    timeframe: state.timeframe,
                    side: decision.side,
                    fill_price: fill.avg_price,
                    size_usdc: fill.cost_usdc,
                    shares: fill.shares,
                    order_id: fill.order_id,
                    slot,
                });
                self.window_slots[slot] = Some(state.window_id);

                // Also track in paper for comparison
                let _ = self.paper.on_tick(state);

                info!(
                    instance = %self.label(),
                    asset = %state.asset,
                    side = %decision.side,
                    fill_price = fill.avg_price,
                    size_usdc = fill.cost_usdc,
                    balance = self.real_balance,
                    "LIVE FILL"
                );

                Some(FillEvent {
                    label: decision.label,
                    asset: state.asset,
                    timeframe: state.timeframe,
                    side: decision.side,
                    strategy_id: decision.strategy_id,
                    fill_price: fill.avg_price,
                    size_usdc: fill.cost_usdc,
                    confidence: decision.confidence,
                    balance_after: self.real_balance,
                })
            }
            Err(e) => {
                warn!(
                    instance = %self.label(),
                    error = %e,
                    asset = %state.asset,
                    side = %decision.side,
                    "LIVE ORDER FAILED"
                );
                None
            }
        }
    }

    fn on_window_close(
        &mut self,
        window_id: WindowId,
        outcome: Side,
        timestamp_ms: u64,
    ) -> Vec<TradeRecord> {
        let mut trades = Vec::new();
        let mut i = 0;
        while i < self.real_positions.len() {
            if self.real_positions[i].window_id != window_id {
                i += 1;
                continue;
            }
            let pos = self.real_positions.swap_remove(i);
            let won = pos.side == outcome;

            let pnl = if won {
                pos.shares * 1.0 - pos.size_usdc
            } else {
                -pos.size_usdc
            };

            self.real_balance += pos.size_usdc + pnl;
            self.real_pnl += pnl;
            self.real_stats.record(pnl);
            self.window_slots[pos.slot] = None;

            let exit_price = if won { 1.0 } else { 0.0 };

            trades.push(TradeRecord {
                window_id,
                asset: pos.asset,
                side: pos.side,
                entry_price: pm_types::ContractPrice::new(pos.fill_price)
                    .unwrap_or(pm_types::ContractPrice::new(0.5).unwrap()),
                exit_price: pm_types::ContractPrice::new(exit_price)
                    .unwrap_or(pm_types::ContractPrice::new(0.5).unwrap()),
                size_usdc: pos.size_usdc,
                pnl: Pnl::new(pnl).unwrap_or(Pnl::ZERO),
                opened_at_ms: 0,
                closed_at_ms: timestamp_ms,
                close_reason: OrderReason::ExpiryClose,
                strategy_id: pm_types::StrategyId::MomentumConfirmation, // TODO: track from decision
            });
        }

        // Also close paper positions for comparison
        let _ = self.paper.on_window_close(window_id, outcome, timestamp_ms);

        trades
    }

    fn balance(&self) -> f64 {
        self.real_balance
    }

    fn stats(&self) -> &InstanceStats {
        &self.real_stats
    }
}
```

- [ ] **Step 4: Create lib.rs**

Create `crates/pm-live/src/lib.rs`:

```rust
//! Live execution module for Polymarket CLOB trading.

pub mod clob;
pub mod instance;

pub use clob::{ClobContext, init_clob_client};
pub use instance::{LiveStrategyInstance, SharedTokenMap, TokenPair};
```

- [ ] **Step 5: Build the crate**

Run: `cargo build -p pm-live`

**Note:** The SDK types may need adjustment based on the actual `polymarket-client-sdk` API. The implementer should check `cargo doc -p polymarket-client-sdk --open` and adjust generic types accordingly.

- [ ] **Step 6: Commit**

```bash
git commit -am "feat(pm-live): add LiveStrategyInstance with CLOB order execution"
```

---

### Task 4: Update builder to support live instances

**Files:**
- Modify: `crates/pm-signal/src/builder.rs`
- Modify: `crates/pm-signal/src/lib.rs`
- Modify: `Cargo.toml` (polymarket-bot binary)

- [ ] **Step 1: Add pm-live dependency to the main binary**

In the root `Cargo.toml` (the binary crate), add:

```toml
[dependencies]
pm-live = { path = "crates/pm-live" }
```

- [ ] **Step 2: Create build function in paper.rs that handles mode**

In `src/paper.rs`, replace the current instance construction with:

```rust
use pm_live::{ClobContext, LiveStrategyInstance, SharedTokenMap, TokenPair, init_clob_client};

// At startup, check if any strategy is "live" and init CLOB if needed
let has_live = cfg.bot.strategies.iter().any(|s| s.mode() == "live");
let clob_ctx: Option<Arc<ClobContext>> = if has_live {
    let ctx = init_clob_client().await
        .context("failed to initialize CLOB client for live trading")?
        .context("POLYMARKET_PRIVATE_KEY required when any strategy has mode=live")?;
    Some(Arc::new(ctx))
} else {
    None
};

// Build token map for live instances
let live_token_map: SharedTokenMap = Arc::new(Mutex::new(HashMap::new()));

// Build instances — paper or live based on mode
let mut instances: Vec<Box<dyn StrategyInstance>> = Vec::new();
for strategy_config in &cfg.bot.strategies {
    let paper = build_paper_instance(strategy_config); // existing function

    if strategy_config.mode() == "live" {
        let clob = clob_ctx.clone()
            .expect("CLOB client required for live mode");
        instances.push(Box::new(LiveStrategyInstance::new(
            paper,
            clob,
            live_token_map.clone(),
        )));
    } else {
        instances.push(Box::new(paper));
    }
}
```

- [ ] **Step 3: Populate live token map from scanner**

In the scanner loop (where `market_mgr.update_markets(markets)` is called), also populate the live token map:

```rust
// After updating market_mgr:
if let Ok(mut map) = live_token_map.lock() {
    for m in &markets {
        map.insert(
            (m.asset, m.timeframe),
            TokenPair {
                up: m.token_id_up.clone(),
                down: m.token_id_down.clone(),
            },
        );
    }
}
```

- [ ] **Step 4: Build and test**

Run: `cargo build --release`
Expected: Compiles. Paper mode works unchanged (no POLYMARKET_PRIVATE_KEY = no live).

- [ ] **Step 5: Commit**

```bash
git commit -am "feat: wire LiveStrategyInstance into paper loop with per-strategy mode"
```

---

### Task 5: Add safety guards and startup validation

**Files:**
- Modify: `crates/pm-live/src/clob.rs`
- Modify: `src/paper.rs`

- [ ] **Step 1: Add startup balance check**

In `clob.rs`, add a function to query on-chain USDC balance:

```rust
pub async fn check_usdc_balance(ctx: &ClobContext) -> Result<f64> {
    // Use the SDK's balance endpoint
    let balance = ctx.client.get_balance_allowance().await
        .context("failed to query USDC balance")?;
    // Extract USDC balance from response
    // The exact field depends on SDK response type
    Ok(0.0) // TODO: extract from balance response
}
```

- [ ] **Step 2: Validate at startup**

In `paper.rs`, after CLOB initialization:

```rust
if let Some(ref ctx) = clob_ctx {
    let balance = check_usdc_balance(ctx).await?;
    let min_required: f64 = cfg.bot.strategies.iter()
        .filter(|s| s.mode() == "live")
        .map(|s| s.balance()) // need to add this accessor
        .sum();

    info!(
        on_chain_balance = format!("${:.2}", balance),
        min_required = format!("${:.2}", min_required),
        "USDC balance check"
    );

    if balance < min_required {
        anyhow::bail!(
            "insufficient USDC balance: ${:.2} on-chain, ${:.2} required by live strategies",
            balance, min_required
        );
    }
}
```

- [ ] **Step 3: Build and test**

Run: `cargo build --release`

- [ ] **Step 4: Commit**

```bash
git commit -am "feat(pm-live): add startup USDC balance validation"
```

---

### Task 6: Update configs and add integration test

**Files:**
- Modify: `config/production.toml`
- Modify: `config/paper-aggressive.toml`
- Create: `tests/live_instance_test.rs` (optional, depends on SDK test support)

- [ ] **Step 1: Add mode examples to production.toml**

```toml
[[bot.strategies]]
type = "momentum_confirmation"
label = "MC-tight"
mode = "paper"          # change to "live" when ready
balance = 250.0
# ...
```

- [ ] **Step 2: Add mode to paper-aggressive.toml**

All strategies keep `mode = "paper"` (default). No changes needed since serde defaults to "paper".

- [ ] **Step 3: Test paper mode still works**

Run: `RUST_LOG=info cargo run --release -- -c config/paper-aggressive.toml paper`
Expected: All 6 strategies start in paper mode, no CLOB errors.

- [ ] **Step 4: Test live mode fails gracefully without key**

Run: Temporarily set one strategy to `mode = "live"` in config, run without env var.
Expected: Clear error message about missing POLYMARKET_PRIVATE_KEY.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat: add mode field to configs, test paper/live mode switching"
```

---

### Task 7: Full verification

- [ ] **Step 1: `cargo build --release`** — must succeed
- [ ] **Step 2: `cargo test --workspace`** — all tests pass
- [ ] **Step 3: `cargo clippy --workspace`** — no errors
- [ ] **Step 4: Paper mode test** — run with all strategies in paper mode, verify fills work
- [ ] **Step 5: Live mode dry run** — with `POLYMARKET_PRIVATE_KEY` set but strategy size = $0.01, verify CLOB auth works

---

## Self-Review

**Spec coverage:**
- LiveStrategyInstance wrapper: Task 3
- CLOB authentication: Task 3 (clob.rs)
- FOK market order: Task 3 (clob.rs)
- Per-strategy mode config: Task 1
- Builder changes: Task 4
- Token map: Task 4
- Safety guards: Task 5
- Error handling: Task 3 (match on fill_result)
- Logging (LIVE FILL vs PAPER FILL): Task 3
- Main loop zero changes: Task 4 (only init code changes)
- Config: Task 6

**Placeholder scan:** The `place_fok_order` function has `// TODO: extract from response` comments for SDK response fields. The implementer must check `cargo doc -p polymarket-client-sdk --open` for exact field names. This is documented clearly, not hidden.

**Type consistency:** `ClobContext`, `LiveFill`, `LiveStrategyInstance`, `TokenPair`, `SharedTokenMap` — used consistently across Tasks 3-5.
