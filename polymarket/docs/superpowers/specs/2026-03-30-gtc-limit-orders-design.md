# GTC Limit Order System Design

## Goal

Replace FOK market orders with GTC limit orders for mid-window strategies (MC-loose) to dramatically improve fill rates (from ~5% to ~80%+), eliminate taker fees (1.56% → 0%), and earn maker rebates. Late-window strategies (LWS) keep FOK for instant execution.

## Problem

The current FOK-only system has a ~5% fill rate. Polymarket crypto up/down 5m/15m orderbooks are thin — market makers pull quotes near expiry, leaving a gap between $0.01 and $0.99. FOK orders at fair prices ($0.40-$0.65) get killed because there's no resting liquidity at those levels.

GTC limit orders rest on the book and wait for someone to take the other side. This is how Polymarket's most profitable bots operate — they post maker orders, pay 0% fees, and collect maker rebates.

## Architecture

Three new components in `pm-live`, plus updates to `LiveStrategyInstance` and config.

### Component 1: User WebSocket Task (`user_ws.rs`)

Connects to `wss://ws-subscriptions-clob.polymarket.com/ws/user` with authenticated credentials. Streams `Order` and `Trade` events into an unbounded mpsc channel consumed by the main loop.

**Events received:**
- `Order { id, msg_type: Placement|Update|Cancellation, size_matched, status }`
- `Trade { id, price, size, status: Matched|Mined|Confirmed|Failed, trader_side: Maker|Taker }`

**Authentication:** API key, secret, passphrase derived from the existing `PrivateKeySigner` via the SDK's credential derivation.

**Lifecycle:** Spawned as a tokio task alongside the market WS. Reconnects with exponential backoff on disconnect. Subscribes to all markets (empty `markets` array).

### Component 2: Order Manager (`order_manager.rs`)

Tracks resting GTC orders, processes fill events, enforces timeouts.

**State per order:**
```rust
struct PendingOrder {
    order_id: String,
    token_id: String,
    asset: Asset,
    timeframe: Timeframe,
    side: Side,
    price: f64,
    original_size_shares: f64,
    filled_shares: f64,
    filled_usdc: f64,
    placed_at_ms: u64,
    timeout_ms: u64,
    strategy_id: StrategyId,
    slot: usize,
    window_id: WindowId,
}
```

**Responsibilities:**
- `place_gtc_order()` — builds GTC limit order with `post_only=true`, signs, posts via SDK, stores as `PendingOrder`
- `on_fill_event()` — updates `filled_shares`/`filled_usdc` from User WS `Order::Update` events
- `check_timeouts()` — called every tick, cancels orders past their `timeout_ms` via `client.cancel_order()`
- `cancel_all_for_window()` — cancels remaining unfilled orders when a window closes
- `drain_filled()` — returns completed orders (fully filled or timed out with partial fill) as `RealPosition` entries for the LiveStrategyInstance

**Cancel flow:** When timeout fires or window closes:
1. Call `client.cancel_order(order_id)` via REST
2. If partially filled, convert to a `RealPosition` with the filled amount
3. If unfilled, discard and free the window slot

### Component 3: Dual-Mode LiveStrategyInstance

`on_tick()` dispatches based on strategy's `order_mode`:

- `order_mode = "fok"` (LWS) — existing FOK path with slippage protection, unchanged
- `order_mode = "gtc"` (MC-loose) — calls `OrderManager::place_gtc_order()`, returns no `FillEvent` immediately. Fill events arrive asynchronously via User WS.

The main paper loop feeds User WS events to the `OrderManager`, which updates pending orders and promotes filled orders to `RealPosition` entries in the `LiveStrategyInstance`.

### Heartbeat

Enable the `heartbeats` feature flag on `polymarket-client-sdk` in `Cargo.toml`. The SDK auto-sends heartbeats every 5 seconds to prevent the CLOB from cancelling all resting orders (10-second timeout).

**Required:** The heartbeat feature starts automatically when the authenticated client is created. No additional code needed.

## Config Changes

```toml
[[bot.strategies]]
type = "late_window_sniper"
label = "LWS-LIVE"
mode = "live"
order_mode = "fok"              # instant execution for last-second entries
# ... existing params ...

[[bot.strategies]]
type = "momentum_confirmation"
label = "MC-loose-LIVE"
mode = "live"
order_mode = "gtc"              # rest on book as maker
gtc_timeout_secs = 120          # cancel unfilled after 2 minutes
# ... existing params ...
```

`order_mode` defaults to `"fok"` for backward compatibility. `gtc_timeout_secs` defaults to 120.

## Order Flow — GTC Path

```
Signal fires (MC-loose evaluate_signal returns Some)
  → LiveStrategyInstance::on_tick() detects order_mode = "gtc"
  → OrderManager::place_gtc_order(token_id, side, size, price, post_only=true)
    → SDK: client.limit_order().price(p).size(s).order_type(GTC).post_only(true).build()
    → SDK: client.sign(signer, signable)
    → SDK: client.post_order(signed)
    → Response: { order_id, status: "live" }
    → Store as PendingOrder with timeout = now + gtc_timeout_secs
  → Return None from on_tick() (no immediate fill)

[Asynchronously via User WS]
  → WsMessage::Order { id, msg_type: Update, size_matched: 1.5 }
  → OrderManager::on_fill_event() updates PendingOrder.filled_shares
  → If fully filled: promote to RealPosition in LiveStrategyInstance
  → Log: "GTC FILL instance=MC-loose-LIVE asset=BTC side=Down price=0.55 shares=1.5"

[On timeout (120s)]
  → OrderManager::check_timeouts() detects expired order
  → SDK: client.cancel_order(order_id)
  → If partially filled: promote partial position to RealPosition
  → If unfilled: discard, free window slot
  → Log: "GTC TIMEOUT instance=MC-loose-LIVE order_id=0x... filled=0.8/1.5 cancelled"

[On window close]
  → OrderManager::cancel_all_for_window(window_id)
  → Cancel any remaining resting orders for this window
  → Resolve positions as normal via on_window_close()
```

## Order Flow — FOK Path (unchanged)

```
Signal fires (LWS evaluate_signal returns Some)
  → LiveStrategyInstance::on_tick() detects order_mode = "fok"
  → place_fok_order(ctx, token_id, side, size, max_price)  [existing code]
  → Immediate fill or rejection
  → Return Some(FillEvent) or None
```

## Fee Impact

| Metric | FOK (current) | GTC (new) |
|--------|--------------|-----------|
| Fee at $0.50 entry | 1.56% taker | **0% maker** |
| Fee at $0.65 entry | 1.20% taker | **0% maker** |
| Maker rebate | None | **~0.3% rebate** |
| Effective cost | -1.2% to -1.6% | **+0.3%** |

Net improvement: **~1.5-1.9% per trade** — on a $2 position that's $0.03-$0.04 saved per trade.

## Dependencies

- `polymarket-client-sdk` v0.4 — add features: `ws`, `heartbeats`
- `tokio` — already used
- `futures-util` — already used (for WS stream)

## Risk Considerations

1. **post_only rejection** — if our price crosses the spread, the order is rejected. This is intentional — the signal will re-evaluate on the next tick.
2. **Stale orders** — the 120s timeout prevents orders from sitting too long. GTD could be used as a belt-and-suspenders expiration but the SDK heartbeat + manual cancel is sufficient.
3. **Heartbeat failure** — if the heartbeat task dies, ALL resting orders are auto-cancelled by the CLOB within 10s. This is a safety feature, not a bug.
4. **Partial fill exposure** — a partially filled order leaves us with a smaller-than-intended position. This is acceptable — we settle whatever accumulated at window close.
5. **User WS disconnect** — on reconnect, poll REST `GET /orders` to sync pending order state. Missing a fill event is recovered by checking `size_matched`.

## Files to Create/Modify

### Create
- `crates/pm-live/src/order_manager.rs` — PendingOrder tracking, timeout, cancel
- `crates/pm-live/src/user_ws.rs` — authenticated User WS connection + event channel

### Modify
- `crates/pm-live/src/clob.rs` — add `place_gtc_order()` alongside existing `place_fok_order()`
- `crates/pm-live/src/instance.rs` — dual-mode dispatch in `on_tick()`, integrate OrderManager
- `crates/pm-live/src/lib.rs` — export new modules
- `crates/pm-live/Cargo.toml` — add `ws` and `heartbeats` features to SDK dependency
- `crates/pm-types/src/config.rs` — add `order_mode` and `gtc_timeout_secs` fields
- `src/paper.rs` — spawn User WS task, feed events to OrderManager, wire into main loop
- `config/live-test.toml` — update with `order_mode` per strategy

## Testing Strategy

1. **Unit tests** — OrderManager: timeout logic, fill accumulation, cancel flow
2. **Integration test** — mock CLOB responses, verify GTC→fill→position lifecycle
3. **Paper validation** — run GTC alongside FOK in same session, compare fill rates
4. **Live smoke test** — small $1 GTC orders on a single asset, verify maker status in User WS trade events (`trader_side: Maker`)
