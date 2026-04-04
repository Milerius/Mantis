# mantis-transport

WebSocket transport ingest layer for the Mantis low-latency financial SDK.

Blocking IO on dedicated pinned threads. No async runtime. No Mutex.

## Architecture

```
VENUE FEEDS                          MANTIS HOT PATH

Polymarket Market WS ──> [SPSC 4096] ──┐
Polymarket User WS   ──> [SPSC 1024] ──┤──> market-state engine
Binance Reference WS ──> [SPSC 8192] ──┤
Timer                ──> [SPSC  256] ──┘
```

Each feed runs on a dedicated CPU-pinned thread. The thread calls
`tungstenite::WebSocket::read()` in a blocking loop, parses venue JSON
into `HotEvent` values, and pushes them into `SpscRingCopy<HotEvent, N>`
queues.

## Thread Model

| Layer | What happens | Allowed |
|---|---|---|
| IO threads (this crate) | WS + TLS, JSON parse, `FixedI64` → `Ticks`/`Lots`, emit `HotEvent` | alloc, strings, serde |
| Owner threads (engine) | book update, signal, order decision | no alloc, no strings, no JSON |

The SPSC ring is the boundary. After `push()`, no strings or decimal types
exist — only `InstrumentId`, `Ticks`, `Lots`, and compact enums.

## Venue Adapters

### Polymarket Market

```rust
use mantis_transport::polymarket::market::*;

let handle = spawn_market_feed(
    PolymarketMarketConfig {
        token_ids: vec![up_token.into(), down_token.into()],
        core_id: Some(1),
        backoff: BackoffConfig::default(),
    },
    |msg| {
        // raw JSON: book, price_change, last_trade_price, etc.
        true // continue
    },
)?;
```

- URL: `wss://ws-subscriptions-clob.polymarket.com/ws/market`
- Heartbeat: text `"PING"` every 10s (mandatory)
- Subscription: `{"assets_ids": [...], "type": "market", "custom_feature_enabled": true}`

### Binance Reference

```rust
use mantis_transport::binance::reference::*;

let handle = spawn_reference_feed(
    BinanceReferenceConfig::default(), // btcusdt@bookTicker
    |msg| {
        // raw JSON: {"e":"bookTicker","b":"67396.70","B":"8.819",...}
        true
    },
)?;
```

- URL: `wss://fstream.binance.com/ws/btcusdt@bookTicker`
- No subscription message needed (stream selection in URL path)
- Uses futures endpoint (`fstream`) for broader geo-availability

## Feed Lifecycle

```
spawn() → connect → subscribe → read loop ──> callback
                                    │
                          on error: reconnect with backoff (1s → 30s, ±12.5% jitter)
                                    │
                        shutdown() → thread joins
```

Monitoring counters (lock-free `AtomicU64`):
- `msg_count` — messages delivered to callback
- `reconnects` — successful reconnections
- `drops` — messages dropped (future: backpressure)

## Socket Tuning

- `TCP_NODELAY` enabled on all connections
- CPU pinning via `core_affinity` (`SocketTuning::core_id`)
- `SO_BUSY_POLL` support on Linux (behind `tuning` feature)

## Features

| Feature | Effect |
|---|---|
| `simd-json` (default) | SIMD-accelerated JSON parsing for Phase C normalization |
| `tuning` (default) | Linux socket tuning (`SO_BUSY_POLL`) |
| `live-tests` | Integration tests against real exchange endpoints |

## Testing

```bash
# Unit + integration tests (local WS echo server)
cargo test -p mantis-transport

# Live endpoint tests (requires network)
cargo test -p mantis-transport --features live-tests -- --nocapture
```

## Safety

`#![deny(unsafe_code)]` on the crate root. The default build has zero `unsafe` blocks. The `tuning` feature enables one audited `unsafe` call (`setsockopt(SO_BUSY_POLL)`) in `tuning.rs`, gated behind `#[expect(unsafe_code)]`.
