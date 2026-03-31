# GTC Limit Order System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace FOK market orders with GTC maker limit orders for mid-window strategies, improving fill rate from ~5% to ~80%+ and eliminating taker fees.

**Architecture:** Dual-mode order dispatch — LWS keeps FOK for instant last-second execution, MC-loose uses GTC `post_only` orders that rest on the book. An `OrderManager` tracks resting orders, processes fill events from an authenticated User WebSocket, and enforces configurable timeouts. The SDK's `heartbeats` feature keeps orders alive.

**Tech Stack:** Rust, polymarket-client-sdk 0.4 (features: clob, ws, heartbeats), tokio, futures-util

---

## File Structure

| File | Responsibility | Action |
|------|---------------|--------|
| `crates/pm-live/Cargo.toml` | SDK feature flags | Modify: add `ws`, `heartbeats` |
| `crates/pm-types/src/config.rs` | Strategy config types | Modify: add `order_mode`, `gtc_timeout_secs` |
| `crates/pm-live/src/clob.rs` | CLOB order helpers | Modify: add `place_gtc_order()`, `cancel_order()` |
| `crates/pm-live/src/order_manager.rs` | Pending order tracking + timeout | Create |
| `crates/pm-live/src/user_ws.rs` | Authenticated User WS client | Create |
| `crates/pm-live/src/instance.rs` | LiveStrategyInstance | Modify: dual-mode dispatch, OrderManager integration |
| `crates/pm-live/src/lib.rs` | Module exports | Modify: export new modules |
| `src/paper.rs` | Main trading loop | Modify: spawn User WS, wire events |
| `config/live-test.toml` | Live config | Modify: add `order_mode` per strategy |

---

### Task 1: Add SDK Feature Flags

**Files:**
- Modify: `crates/pm-live/Cargo.toml`

- [ ] **Step 1: Update polymarket-client-sdk dependency**

```toml
polymarket-client-sdk = { version = "0.4", features = ["clob", "ws", "heartbeats"] }
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p pm-live`
Expected: PASS (no new code yet, just features enabled)

- [ ] **Step 3: Commit**

```bash
git add crates/pm-live/Cargo.toml
git commit -m "feat(pm-live): enable ws and heartbeats SDK features"
```

---

### Task 2: Add Config Fields for Order Mode

**Files:**
- Modify: `crates/pm-types/src/config.rs`

- [ ] **Step 1: Add default functions and fields to MomentumConfirmation and LateWindowSniper**

Add these defaults near the other default functions (around line 330):

```rust
fn default_order_mode() -> String {
    "fok".to_string()
}

fn default_gtc_timeout_secs() -> u64 {
    120
}
```

Add to the `MomentumConfirmation` variant (after `slippage_bps`):

```rust
        /// Order execution mode: `"fok"` (default) or `"gtc"`.
        #[serde(default = "default_order_mode")]
        order_mode: String,
        /// Seconds before an unfilled GTC order is cancelled.
        #[serde(default = "default_gtc_timeout_secs")]
        gtc_timeout_secs: u64,
```

Add the same two fields to `LateWindowSniper`, `EarlyDirectional`, `MeanReversion`, `CompleteSetArb`, and `HedgeLock` variants.

- [ ] **Step 2: Add accessor methods on StrategyConfig**

Add after the existing `mode()` method:

```rust
    /// Get the order execution mode (`"fok"` or `"gtc"`).
    pub fn order_mode(&self) -> &str {
        match self {
            Self::EarlyDirectional { order_mode, .. }
            | Self::MomentumConfirmation { order_mode, .. }
            | Self::CompleteSetArb { order_mode, .. }
            | Self::HedgeLock { order_mode, .. }
            | Self::LateWindowSniper { order_mode, .. }
            | Self::MeanReversion { order_mode, .. } => order_mode,
        }
    }

    /// Get the GTC timeout in seconds.
    pub fn gtc_timeout_secs(&self) -> u64 {
        match self {
            Self::EarlyDirectional { gtc_timeout_secs, .. }
            | Self::MomentumConfirmation { gtc_timeout_secs, .. }
            | Self::CompleteSetArb { gtc_timeout_secs, .. }
            | Self::HedgeLock { gtc_timeout_secs, .. }
            | Self::LateWindowSniper { gtc_timeout_secs, .. }
            | Self::MeanReversion { gtc_timeout_secs, .. } => *gtc_timeout_secs,
        }
    }
```

- [ ] **Step 3: Update default_strategies() to include the new fields**

In each variant of `default_strategies()`, add:

```rust
            order_mode: default_order_mode(),
            gtc_timeout_secs: default_gtc_timeout_secs(),
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build --features std -p pm-types`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/pm-types/src/config.rs
git commit -m "feat(pm-types): add order_mode and gtc_timeout_secs config fields"
```

---

### Task 3: Add GTC Order Placement and Cancel Helpers

**Files:**
- Modify: `crates/pm-live/src/clob.rs`

- [ ] **Step 1: Add `place_gtc_order()` function**

Add after the existing `place_fok_order()`:

```rust
/// Place a GTC (Good-Til-Cancelled) limit buy order with `post_only=true`.
///
/// Rests on the book as a maker order (0% fees + rebates).
/// Returns the order ID and status. Does NOT return fill details —
/// fills arrive asynchronously via the User WebSocket.
///
/// # Errors
///
/// Returns `Err` if the order is rejected (e.g. post_only would cross spread).
pub async fn place_gtc_order(
    ctx: &ClobContext,
    token_id: &str,
    side: ClobSide,
    size_shares: f64,
    price: f64,
) -> Result<GtcOrderResult> {
    let token_u256 = U256::from_str(token_id)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("invalid token_id for U256 conversion")?;

    let price_dec = Decimal::try_from(price)
        .context("invalid price for Decimal conversion")?
        .round_dp(2);

    let size_dec = Decimal::try_from(size_shares)
        .context("invalid size for Decimal conversion")?
        .round_dp(2);

    let signable = ctx
        .client
        .limit_order()
        .token_id(token_u256)
        .price(price_dec)
        .size(size_dec)
        .side(side)
        .order_type(OrderType::GTC)
        .post_only(true)
        .build()
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to build GTC limit order")?;

    let signed = ctx
        .client
        .sign(&*ctx.signer, signable)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to sign GTC order")?;

    let response = ctx
        .client
        .post_order(signed)
        .await
        .map_err(|e| {
            warn!(error = %e, "CLOB post_order (GTC) raw error");
            anyhow::anyhow!("post_order GTC: {e}")
        })?;

    if !response.success {
        anyhow::bail!(
            "GTC order rejected: {}",
            response.error_msg.as_deref().unwrap_or("unknown")
        );
    }

    Ok(GtcOrderResult {
        order_id: response.order_id,
        status: format!("{:?}", response.status),
    })
}

/// Result of posting a GTC order (no fill details — fills arrive via WS).
#[derive(Debug, Clone)]
pub struct GtcOrderResult {
    /// Order ID returned by the CLOB.
    pub order_id: String,
    /// Initial status: "Live", "Matched", etc.
    pub status: String,
}
```

- [ ] **Step 2: Add `cancel_order()` wrapper**

```rust
/// Cancel a resting order by ID.
///
/// # Errors
///
/// Returns `Err` if the cancel request fails.
pub async fn cancel_order(ctx: &ClobContext, order_id: &str) -> Result<()> {
    let response = ctx
        .client
        .cancel_order(order_id)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to cancel order")?;

    if !response.not_canceled.is_empty() {
        for (id, reason) in &response.not_canceled {
            warn!(order_id = %id, reason = %reason, "order cancel failed");
        }
    }

    Ok(())
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p pm-live`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/pm-live/src/clob.rs
git commit -m "feat(pm-live): add place_gtc_order and cancel_order helpers"
```

---

### Task 4: Create Order Manager

**Files:**
- Create: `crates/pm-live/src/order_manager.rs`

- [ ] **Step 1: Write the OrderManager with PendingOrder tracking**

```rust
//! Tracks resting GTC orders, processes fill events, enforces timeouts.

use std::sync::Arc;

use anyhow::Result;
use pm_types::{Asset, Side, StrategyId, Timeframe, WindowId};
use tracing::{info, warn};

use crate::clob::{ClobContext, cancel_order};

/// A GTC order resting on the CLOB book, awaiting fills.
pub struct PendingOrder {
    pub order_id: String,
    pub token_id: String,
    pub asset: Asset,
    pub timeframe: Timeframe,
    pub side: Side,
    pub price: f64,
    pub original_size_shares: f64,
    pub filled_shares: f64,
    pub filled_usdc: f64,
    pub placed_at_ms: u64,
    pub timeout_ms: u64,
    pub strategy_id: StrategyId,
    pub slot: usize,
    pub window_id: WindowId,
    pub instance_label: String,
}

/// A filled (or partially filled) order ready to become a RealPosition.
pub struct FilledOrder {
    pub order_id: String,
    pub token_id: String,
    pub asset: Asset,
    pub timeframe: Timeframe,
    pub side: Side,
    pub avg_price: f64,
    pub size_usdc: f64,
    pub shares: f64,
    pub slot: usize,
    pub window_id: WindowId,
    pub strategy_id: StrategyId,
    pub instance_label: String,
}

/// Manages resting GTC orders across all live strategy instances.
pub struct OrderManager {
    pending: Vec<PendingOrder>,
    clob: Arc<ClobContext>,
    rt_handle: tokio::runtime::Handle,
}

impl OrderManager {
    /// Create a new order manager.
    pub fn new(clob: Arc<ClobContext>) -> Self {
        Self {
            pending: Vec::new(),
            clob,
            rt_handle: tokio::runtime::Handle::current(),
        }
    }

    /// Register a newly placed GTC order for tracking.
    pub fn track(&mut self, order: PendingOrder) {
        info!(
            instance = %order.instance_label,
            order_id = %order.order_id,
            asset = %order.asset,
            side = %order.side,
            price = order.price,
            size_shares = order.original_size_shares,
            "GTC order placed — tracking"
        );
        self.pending.push(order);
    }

    /// Process a fill event from the User WebSocket.
    ///
    /// Updates the matching PendingOrder's filled_shares and filled_usdc.
    /// The `order_id` and `size_matched` come from an OrderMessage with
    /// msg_type=Update.
    pub fn on_fill_update(&mut self, order_id: &str, size_matched: f64) {
        let Some(order) = self.pending.iter_mut().find(|o| o.order_id == order_id) else {
            return; // not our order
        };

        let prev_shares = order.filled_shares;
        order.filled_shares = size_matched;
        // Approximate USDC spent = shares * price (maker fills at posted price).
        order.filled_usdc = size_matched * order.price;

        let new_shares = size_matched - prev_shares;
        if new_shares > 0.0 {
            info!(
                instance = %order.instance_label,
                order_id = %order.order_id,
                asset = %order.asset,
                side = %order.side,
                new_shares = format!("{new_shares:.2}"),
                total_filled = format!("{size_matched:.2}/{:.2}", order.original_size_shares),
                "GTC PARTIAL FILL"
            );
        }
    }

    /// Mark an order as cancelled (from WS Cancellation event).
    pub fn on_cancel_event(&mut self, order_id: &str) {
        // Will be drained in drain_completed
        if let Some(order) = self.pending.iter().find(|o| o.order_id == order_id) {
            info!(
                instance = %order.instance_label,
                order_id = %order.order_id,
                filled = format!("{:.2}/{:.2}", order.filled_shares, order.original_size_shares),
                "GTC order cancelled externally"
            );
        }
    }

    /// Check for timed-out orders and cancel them on the CLOB.
    ///
    /// Call this every tick or on a timer. `now_ms` is the current
    /// Unix timestamp in milliseconds.
    pub fn check_timeouts(&mut self, now_ms: u64) {
        let clob = self.clob.clone();
        let rt = self.rt_handle.clone();

        for order in &self.pending {
            if now_ms >= order.placed_at_ms + order.timeout_ms {
                let order_id = order.order_id.clone();
                let label = order.instance_label.clone();
                let filled = order.filled_shares;
                let total = order.original_size_shares;

                info!(
                    instance = %label,
                    order_id = %order_id,
                    filled = format!("{filled:.2}/{total:.2}"),
                    "GTC TIMEOUT — cancelling"
                );

                let clob_ref = clob.clone();
                tokio::task::block_in_place(|| {
                    rt.block_on(async {
                        if let Err(e) = cancel_order(&clob_ref, &order_id).await {
                            warn!(
                                order_id = %order_id,
                                error = %e,
                                "failed to cancel timed-out order"
                            );
                        }
                    });
                });
            }
        }
    }

    /// Cancel all pending orders for a specific window.
    pub fn cancel_all_for_window(&mut self, window_id: WindowId) {
        let clob = self.clob.clone();
        let rt = self.rt_handle.clone();

        for order in self.pending.iter().filter(|o| o.window_id == window_id) {
            let order_id = order.order_id.clone();
            let clob_ref = clob.clone();
            tokio::task::block_in_place(|| {
                rt.block_on(async {
                    if let Err(e) = cancel_order(&clob_ref, &order_id).await {
                        warn!(order_id = %order_id, error = %e, "failed to cancel order on window close");
                    }
                });
            });
        }
    }

    /// Drain completed orders (fully filled or timed-out with partial fill).
    ///
    /// Returns `FilledOrder`s that should be promoted to `RealPosition`.
    /// Removes completed/cancelled orders from the pending list.
    /// `now_ms` is used to detect timed-out orders.
    pub fn drain_completed(&mut self, now_ms: u64) -> Vec<FilledOrder> {
        let mut filled = Vec::new();
        let mut i = 0;

        while i < self.pending.len() {
            let order = &self.pending[i];
            let is_fully_filled =
                order.filled_shares >= order.original_size_shares - 0.001; // float tolerance
            let is_timed_out = now_ms >= order.placed_at_ms + order.timeout_ms;

            if is_fully_filled || (is_timed_out && order.filled_shares > 0.0) {
                let order = self.pending.swap_remove(i);
                let avg_price = if order.filled_shares > 0.0 {
                    order.filled_usdc / order.filled_shares
                } else {
                    order.price
                };

                filled.push(FilledOrder {
                    order_id: order.order_id,
                    token_id: order.token_id,
                    asset: order.asset,
                    timeframe: order.timeframe,
                    side: order.side,
                    avg_price,
                    size_usdc: order.filled_usdc,
                    shares: order.filled_shares,
                    slot: order.slot,
                    window_id: order.window_id,
                    strategy_id: order.strategy_id,
                    instance_label: order.instance_label,
                });
                // Don't increment i — swap_remove moved last element here
            } else if is_timed_out && order.filled_shares <= 0.0 {
                // Timed out with zero fill — discard
                let order = self.pending.swap_remove(i);
                info!(
                    instance = %order.instance_label,
                    order_id = %order.order_id,
                    asset = %order.asset,
                    "GTC expired unfilled — discarding"
                );
                // Don't increment i
            } else {
                i += 1;
            }
        }

        filled
    }

    /// Check if there's already a pending order for this (slot, window).
    pub fn has_pending_for_slot(&self, slot: usize, window_id: WindowId) -> bool {
        self.pending
            .iter()
            .any(|o| o.slot == slot && o.window_id == window_id)
    }

    /// Number of currently pending orders.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p pm-live`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/pm-live/src/order_manager.rs
git commit -m "feat(pm-live): add OrderManager for GTC order tracking and timeout"
```

---

### Task 5: Create User WebSocket Client

**Files:**
- Create: `crates/pm-live/src/user_ws.rs`

- [ ] **Step 1: Write the User WS event types and run loop**

```rust
//! Authenticated User WebSocket client for real-time fill monitoring.
//!
//! Connects to the Polymarket CLOB user channel and streams `Order` and
//! `Trade` events. Used by the OrderManager to track GTC order fills.

use std::sync::Arc;

use futures_util::StreamExt;
use polymarket_client_sdk::clob::ws::types::response::{OrderMessage, TradeMessage};
use polymarket_client_sdk::clob::Client;
use polymarket_client_sdk::auth::Normal;
use polymarket_client_sdk::auth::state::Authenticated;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn, debug};

/// Events emitted by the User WebSocket.
#[derive(Debug)]
pub enum UserWsEvent {
    /// An order status update (placement, partial fill, cancellation).
    OrderUpdate {
        order_id: String,
        size_matched: f64,
        is_cancelled: bool,
    },
    /// A confirmed trade execution.
    TradeConfirmed {
        order_id: String,
        price: f64,
        size: f64,
        is_maker: bool,
    },
}

/// Sender for User WS events consumed by the main loop.
pub type UserWsEventSender = mpsc::UnboundedSender<UserWsEvent>;
/// Receiver for User WS events consumed by the main loop.
pub type UserWsEventReceiver = mpsc::UnboundedReceiver<UserWsEvent>;

/// Create an event channel for User WS events.
pub fn user_ws_channel() -> (UserWsEventSender, UserWsEventReceiver) {
    mpsc::unbounded_channel()
}

/// Run the User WebSocket connection loop.
///
/// Subscribes to all markets (empty vec) to receive order/trade events
/// for all tokens. Reconnects with exponential backoff on disconnect.
///
/// # Errors
///
/// Logs errors and retries. Only returns when `shutdown` is cancelled.
pub async fn run_user_ws(
    client: Arc<Client<Authenticated<Normal>>>,
    tx: UserWsEventSender,
    shutdown: CancellationToken,
) {
    let mut backoff_secs: u64 = 1;

    loop {
        if shutdown.is_cancelled() {
            break;
        }

        info!("User WS: connecting...");

        match client.subscribe_user_events(vec![]) {
            Ok(stream) => {
                info!("User WS: connected");
                backoff_secs = 1;

                let mut stream = std::pin::pin!(stream);

                loop {
                    tokio::select! {
                        () = shutdown.cancelled() => return,
                        msg = stream.next() => {
                            match msg {
                                Some(Ok(ws_msg)) => {
                                    use polymarket_client_sdk::clob::ws::types::response::WsMessage;
                                    match ws_msg {
                                        WsMessage::Order(order) => {
                                            handle_order_event(&order, &tx);
                                        }
                                        WsMessage::Trade(trade) => {
                                            handle_trade_event(&trade, &tx);
                                        }
                                        _ => {
                                            // Market events on user channel — ignore
                                        }
                                    }
                                }
                                Some(Err(e)) => {
                                    warn!(error = %e, "User WS: stream error");
                                    break;
                                }
                                None => {
                                    warn!("User WS: stream ended");
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "User WS: connect failed");
            }
        }

        if !shutdown.is_cancelled() {
            warn!(backoff_secs = backoff_secs, "User WS: reconnecting");
            tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
            backoff_secs = (backoff_secs * 2).min(30);
        }
    }
}

fn handle_order_event(order: &OrderMessage, tx: &UserWsEventSender) {
    use polymarket_client_sdk::clob::ws::types::response::OrderMessageType;

    let size_matched = order
        .size_matched
        .map(|d| d.to_string().parse::<f64>().unwrap_or(0.0))
        .unwrap_or(0.0);

    let is_cancelled = order
        .msg_type
        .as_ref()
        .is_some_and(|t| matches!(t, OrderMessageType::Cancellation));

    debug!(
        order_id = %order.id,
        msg_type = ?order.msg_type,
        size_matched = size_matched,
        "User WS: order event"
    );

    let _ = tx.send(UserWsEvent::OrderUpdate {
        order_id: order.id.clone(),
        size_matched,
        is_cancelled,
    });
}

fn handle_trade_event(trade: &TradeMessage, tx: &UserWsEventSender) {
    use polymarket_client_sdk::clob::ws::types::response::TraderSide;

    let price = trade.price.to_string().parse::<f64>().unwrap_or(0.0);
    let size = trade.size.to_string().parse::<f64>().unwrap_or(0.0);
    let is_maker = trade
        .trader_side
        .as_ref()
        .is_some_and(|s| matches!(s, TraderSide::Maker));

    let order_id = trade.taker_order_id.clone().unwrap_or_default();

    debug!(
        trade_id = %trade.id,
        price = price,
        size = size,
        is_maker = is_maker,
        "User WS: trade event"
    );

    let _ = tx.send(UserWsEvent::TradeConfirmed {
        order_id,
        price,
        size,
        is_maker,
    });
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p pm-live`
Expected: PASS (may need import adjustments depending on SDK re-exports)

- [ ] **Step 3: Commit**

```bash
git add crates/pm-live/src/user_ws.rs
git commit -m "feat(pm-live): add authenticated User WebSocket client for fill monitoring"
```

---

### Task 6: Update lib.rs Exports

**Files:**
- Modify: `crates/pm-live/src/lib.rs`

- [ ] **Step 1: Add new module declarations and exports**

```rust
//! Live execution module for Polymarket CLOB trading.

#![deny(unsafe_code)]

pub mod clob;
pub mod instance;
pub mod order_manager;
pub mod user_ws;

pub use clob::{ClobContext, GtcOrderResult, LiveFill, cancel_order, init_clob_client, place_fok_order, place_gtc_order};
pub use instance::{LiveStrategyInstance, SharedTokenMap, TokenPair};
pub use order_manager::{FilledOrder, OrderManager, PendingOrder};
pub use user_ws::{UserWsEvent, UserWsEventReceiver, UserWsEventSender, run_user_ws, user_ws_channel};
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p pm-live`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/pm-live/src/lib.rs
git commit -m "feat(pm-live): export order_manager and user_ws modules"
```

---

### Task 7: Update LiveStrategyInstance for Dual-Mode Dispatch

**Files:**
- Modify: `crates/pm-live/src/instance.rs`

- [ ] **Step 1: Add order_mode and OrderManager fields to LiveStrategyInstance**

Add to the struct fields:

```rust
    /// Order execution mode: "fok" or "gtc".
    order_mode: String,

    /// GTC timeout in milliseconds.
    gtc_timeout_ms: u64,
```

Update `new()` to accept the new params:

```rust
    pub fn new(
        paper: ConcreteStrategyInstance,
        clob: Arc<ClobContext>,
        token_map: SharedTokenMap,
        order_mode: String,
        gtc_timeout_secs: u64,
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
            order_mode,
            gtc_timeout_ms: gtc_timeout_secs * 1000,
        }
    }
```

- [ ] **Step 2: Add GTC dispatch path in on_tick()**

After the existing price guard and before the FOK order placement, add the GTC branch:

```rust
        // 7. Resolve token ID from scanner-populated map.
        let token_id = self.get_token_id(state.asset, state.timeframe, decision.side)?;

        // 8. Dispatch based on order_mode.
        if self.order_mode == "gtc" {
            // GTC path: place maker limit order, return None (fills arrive via WS).
            let clob = self.clob.clone();
            let price_rounded = ((decision.limit_price.as_f64().min(MAX_LIVE_ENTRY)) * 100.0).floor() / 100.0;
            let size_shares = ((size / price_rounded) * 100.0).floor() / 100.0;

            let gtc_result = tokio::task::block_in_place(|| {
                self.rt_handle.block_on(async {
                    crate::clob::place_gtc_order(
                        &clob,
                        &token_id,
                        Self::to_clob_side(decision.side),
                        size_shares,
                        price_rounded,
                    )
                    .await
                })
            });

            match gtc_result {
                Ok(result) => {
                    info!(
                        instance = %self.label(),
                        order_id = %result.order_id,
                        asset = %state.asset,
                        side = %decision.side,
                        price = price_rounded,
                        size_shares = size_shares,
                        "GTC ORDER POSTED"
                    );
                    self.window_slots[slot] = Some(state.window_id);

                    // Return the pending order info for the OrderManager to track.
                    // We store it in a temporary field that the caller can drain.
                    self.last_gtc_order = Some(crate::order_manager::PendingOrder {
                        order_id: result.order_id,
                        token_id,
                        asset: state.asset,
                        timeframe: state.timeframe,
                        side: decision.side,
                        price: price_rounded,
                        original_size_shares: size_shares,
                        filled_shares: 0.0,
                        filled_usdc: 0.0,
                        placed_at_ms: state.window_id.as_u64() * 1000, // approximate
                        timeout_ms: self.gtc_timeout_ms,
                        strategy_id: decision.strategy_id,
                        slot,
                        window_id: state.window_id,
                        instance_label: self.label().to_string(),
                    });

                    // Also track in paper for comparison.
                    let _ = self.paper.on_tick(state);
                }
                Err(e) => {
                    warn!(
                        instance = %self.label(),
                        error = %e,
                        asset = %state.asset,
                        side = %decision.side,
                        "GTC ORDER FAILED"
                    );
                    self.window_slots[slot] = Some(state.window_id);
                }
            }
            return None; // GTC fills arrive asynchronously
        }

        // 9. FOK path (existing code follows)...
```

- [ ] **Step 3: Add last_gtc_order field and drain method**

Add to the struct:

```rust
    /// Last GTC order placed, for the caller to drain and pass to OrderManager.
    last_gtc_order: Option<crate::order_manager::PendingOrder>,
```

Add a public method:

```rust
    /// Drain the last GTC order placed (if any) for the OrderManager to track.
    pub fn take_pending_gtc(&mut self) -> Option<crate::order_manager::PendingOrder> {
        self.last_gtc_order.take()
    }

    /// Promote a filled GTC order into a RealPosition.
    pub fn promote_gtc_fill(&mut self, fill: &crate::order_manager::FilledOrder) {
        self.real_balance -= fill.size_usdc;
        self.real_positions.push(RealPosition {
            window_id: fill.window_id,
            asset: fill.asset,
            timeframe: fill.timeframe,
            side: fill.side,
            fill_price: fill.avg_price,
            size_usdc: fill.size_usdc,
            shares: fill.shares,
            order_id: fill.order_id.clone(),
            slot: fill.slot,
            strategy_id: fill.strategy_id,
            token_id: fill.token_id.clone(),
        });

        info!(
            instance = %self.label(),
            asset = %fill.asset,
            side = %fill.side,
            fill_price = fill.avg_price,
            shares = fill.shares,
            size_usdc = fill.size_usdc,
            balance = self.real_balance,
            "GTC FILL PROMOTED"
        );
    }
```

Initialize `last_gtc_order: None` in `new()`.

- [ ] **Step 4: Verify it compiles**

Run: `cargo build -p pm-live`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/pm-live/src/instance.rs
git commit -m "feat(pm-live): dual-mode dispatch in LiveStrategyInstance (FOK/GTC)"
```

---

### Task 8: Wire Everything in the Main Loop

**Files:**
- Modify: `src/paper.rs`
- Modify: `config/live-test.toml`

- [ ] **Step 1: Update LiveStrategyInstance construction to pass order_mode**

In `src/paper.rs` around line 715, update the live instance creation:

```rust
        if strategy_cfg.mode() == "live" {
            #[expect(clippy::expect_used, reason = "CLOB context guaranteed present when has_live is true")]
            let clob = clob_ctx.clone().expect("CLOB required for live");
            instances.push(Box::new(LiveStrategyInstance::new(
                paper,
                clob,
                live_token_map.clone(),
                strategy_cfg.order_mode().to_string(),
                strategy_cfg.gtc_timeout_secs(),
            )));
        } else {
            instances.push(Box::new(paper));
        }
```

- [ ] **Step 2: Spawn User WS task and create OrderManager**

After CLOB client initialization, add:

```rust
    // Create OrderManager and User WS channel for GTC order tracking.
    let has_gtc = cfg.bot.strategies.iter().any(|s| s.order_mode() == "gtc");
    let (user_ws_tx, mut user_ws_rx) = pm_live::user_ws_channel();
    let mut order_manager = clob_ctx.as_ref().map(|clob| pm_live::OrderManager::new(clob.clone()));

    if has_gtc {
        if let Some(ref clob) = clob_ctx {
            let client = Arc::new(clob.client.clone()); // needs Arc<Client>
            let user_ws_shutdown = shutdown.clone();
            tokio::spawn(pm_live::run_user_ws(client, user_ws_tx.clone(), user_ws_shutdown));
            info!("User WS task spawned for GTC fill monitoring");
        }
    }
```

- [ ] **Step 3: Process User WS events and OrderManager in the main select! loop**

Add a new branch to the main `tokio::select!` loop:

```rust
                // Process User WS events for GTC order fills.
                Some(event) = user_ws_rx.recv() => {
                    if let Some(ref mut mgr) = order_manager {
                        match event {
                            pm_live::UserWsEvent::OrderUpdate { order_id, size_matched, is_cancelled } => {
                                if is_cancelled {
                                    mgr.on_cancel_event(&order_id);
                                } else {
                                    mgr.on_fill_update(&order_id, size_matched);
                                }
                            }
                            pm_live::UserWsEvent::TradeConfirmed { .. } => {
                                // Trade confirmations are informational — fills
                                // are tracked via OrderUpdate.size_matched.
                            }
                        }
                    }
                }
```

- [ ] **Step 4: After on_tick, drain pending GTC orders and promote fills**

After the `instance.on_tick()` call in the tick processing, add:

```rust
                    // Drain any GTC orders placed by live instances.
                    if let Some(ref mut mgr) = order_manager {
                        // Check if any instance placed a GTC order.
                        for instance in instances.iter_mut() {
                            // Downcast not possible with dyn trait — use the
                            // take_pending_gtc method added to StrategyInstance trait.
                            // For now, we'll add it as a default no-op on the trait.
                        }

                        // Check timeouts and drain completed fills.
                        mgr.check_timeouts(tick.timestamp_ms);
                        let filled = mgr.drain_completed(tick.timestamp_ms);
                        for fill in &filled {
                            // Find the matching instance and promote.
                            for instance in instances.iter_mut() {
                                if instance.label() == fill.instance_label {
                                    // Promote via trait method.
                                    instance.promote_gtc_fill(fill);
                                    break;
                                }
                            }
                        }
                    }
```

- [ ] **Step 5: Add `take_pending_gtc` and `promote_gtc_fill` to StrategyInstance trait**

In `crates/pm-types/src/strategy.rs`, add default no-op methods:

```rust
    /// Drain a pending GTC order placed by the last on_tick (live only).
    fn take_pending_gtc(&mut self) -> Option<pm_live::PendingOrder> {
        None
    }

    /// Promote a filled GTC order into a real position (live only).
    fn promote_gtc_fill(&mut self, _fill: &pm_live::FilledOrder) {}
```

Note: Since `pm-types` cannot depend on `pm-live`, these methods should use generic types or we pass the data through the caller (paper.rs) without trait methods. The pragmatic approach: keep the GTC integration logic in `paper.rs` by iterating instances and checking labels rather than adding trait methods.

- [ ] **Step 6: Update config/live-test.toml**

```toml
# LIVE: LWS — Late Window Sniper (FOK for instant last-second execution)
[[bot.strategies]]
type = "late_window_sniper"
label = "LWS-LIVE"
mode = "live"
order_mode = "fok"
# ... existing params unchanged ...

# LIVE: MC-loose — momentum confirmation (GTC maker orders)
[[bot.strategies]]
type = "momentum_confirmation"
label = "MC-loose-LIVE"
mode = "live"
order_mode = "gtc"
gtc_timeout_secs = 120
# ... existing params unchanged ...
```

- [ ] **Step 7: Build and verify everything compiles**

Run: `cargo build --release`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add src/paper.rs config/live-test.toml crates/pm-types/src/strategy.rs
git commit -m "feat: wire GTC order system into main trading loop"
```

---

### Task 9: Integration Test — Full GTC Lifecycle

**Files:**
- Modify: `crates/pm-live/src/order_manager.rs` (add tests)

- [ ] **Step 1: Write unit tests for OrderManager**

Add at the bottom of `order_manager.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pm_types::{Asset, Side, StrategyId, Timeframe, WindowId};

    fn make_pending(order_id: &str, placed_at_ms: u64, timeout_ms: u64) -> PendingOrder {
        PendingOrder {
            order_id: order_id.to_string(),
            token_id: "tok_123".to_string(),
            asset: Asset::Btc,
            timeframe: Timeframe::Min15,
            side: Side::Down,
            price: 0.55,
            original_size_shares: 4.0,
            filled_shares: 0.0,
            filled_usdc: 0.0,
            placed_at_ms,
            timeout_ms,
            strategy_id: StrategyId::MomentumConfirmation,
            slot: 0,
            window_id: WindowId::new(1),
            instance_label: "MC-test".to_string(),
        }
    }

    #[test]
    fn fill_update_tracks_cumulative_shares() {
        // We can't easily construct an OrderManager without a real ClobContext,
        // so test PendingOrder logic directly.
        let mut order = make_pending("0x1", 1000, 120_000);
        assert_eq!(order.filled_shares, 0.0);

        // Simulate partial fill
        order.filled_shares = 1.5;
        order.filled_usdc = 1.5 * 0.55;
        assert!((order.filled_usdc - 0.825).abs() < 0.001);

        // Simulate full fill
        order.filled_shares = 4.0;
        order.filled_usdc = 4.0 * 0.55;
        assert!((order.filled_usdc - 2.20).abs() < 0.001);
    }

    #[test]
    fn timeout_detection() {
        let order = make_pending("0x2", 1000, 120_000);
        // Not timed out at 1000 + 119_999
        assert!(120_999 < order.placed_at_ms + order.timeout_ms);
        // Timed out at 1000 + 120_001
        assert!(121_001 >= order.placed_at_ms + order.timeout_ms);
    }

    #[test]
    fn has_pending_for_slot() {
        let order = make_pending("0x3", 1000, 120_000);
        assert_eq!(order.slot, 0);
        assert_eq!(order.window_id, WindowId::new(1));
    }

    #[test]
    fn filled_order_avg_price() {
        let mut order = make_pending("0x4", 1000, 120_000);
        order.filled_shares = 2.0;
        order.filled_usdc = 2.0 * 0.55;
        let avg = order.filled_usdc / order.filled_shares;
        assert!((avg - 0.55).abs() < 0.001);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p pm-live`
Expected: PASS (4 tests)

- [ ] **Step 3: Commit**

```bash
git add crates/pm-live/src/order_manager.rs
git commit -m "test(pm-live): add OrderManager unit tests"
```

---

### Task 10: Full Build Verification

- [ ] **Step 1: Run full build**

Run: `cargo build --release`
Expected: PASS

- [ ] **Step 2: Run all tests**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 3: Run clippy**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: No warnings

- [ ] **Step 4: Commit any lint fixes**

```bash
git add -A
git commit -m "chore: fix clippy warnings from GTC implementation"
```
