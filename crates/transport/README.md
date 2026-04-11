# mantis-transport

WebSocket transport, timer, and feed monitoring infrastructure for the Mantis low-latency financial SDK.

Blocking IO on dedicated pinned threads. No async runtime. No Mutex.

## Architecture

```
VENUE FEEDS                          MANTIS HOT PATH

Polymarket Market WS ──> callback ──┐
Polymarket User WS   ──> callback ──┤──> venue decoder ──> [SPSC] ──> engine
Binance Reference WS ──> callback ──┤
Timer thread         ──> [SPSC  256] ──────────────────────────────┘
                                        │
FeedMonitor ◄── event_count atomics ────┘  (stale feed detection)
```

Each feed runs on a dedicated CPU-pinned thread. The thread calls
`tungstenite::WebSocket::read()` in a blocking loop and delivers raw
`&mut [u8]` buffers to an `FnMut` callback. Venue-specific JSON parsing
and `HotEvent` emission live in separate crates (`mantis-binance`,
`mantis-polymarket`), not in transport.

## Thread Model

| Layer | What happens | Allowed |
|---|---|---|
| IO threads (this crate) | WS + TLS, deliver raw bytes via callback | alloc, strings, network |
| Venue decoders (separate crates) | JSON parse, `FixedI64` -> `Ticks`/`Lots`, emit `HotEvent` | alloc, serde |
| Owner threads (engine) | book update, signal, order decision | no alloc, no strings, no JSON |

The SPSC ring is the boundary. After `push()`, no strings or decimal types
exist -- only `InstrumentId`, `Ticks`, `Lots`, and compact enums.

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
        // raw &mut [u8]: book, price_change, last_trade_price, etc.
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
        // raw &mut [u8]: {"e":"bookTicker","b":"67396.70","B":"8.819",...}
        true
    },
)?;
```

- URL: `wss://fstream.binance.com/ws/btcusdt@bookTicker`
- No subscription message needed (stream selection in URL path)
- Uses futures endpoint (`fstream`) for broader geo-availability

## Timer Thread

The timer thread emits periodic `HotEvent::Timer` and `HotEvent::Heartbeat` events at configurable intervals. It runs on a dedicated thread with no WS connection.

```rust
use mantis_transport::{TimerConfig, TimerThread};

let timer = TimerThread::spawn(
    TimerConfig {
        name: "timer".into(),
        tick_interval: Duration::from_millis(100),
        heartbeat_interval: Duration::from_secs(1),
        source_id: SourceId::from_raw(99),
        core_id: Some(3),
    },
    |event| { let _ = ring.try_push(event); },
)?;
```

## Feed Monitor

`FeedMonitor` passively tracks `event_count` atomics from feed spawn wrappers and detects feeds that have stopped producing events. The engine calls `check_all()` on each `Timer(Periodic)` event.

```rust
use mantis_transport::FeedMonitor;

let mut monitor = FeedMonitor::new();
monitor.register(source_id, event_count_arc)?;

// On each timer tick:
let stale_count = monitor.check_all();
if stale_count > 0 {
    for info in monitor.stale_feeds() {
        // info.source_id, info.last_event_count
    }
}
```

## Feed Lifecycle

```text
spawn() -> connect -> subscribe -> read loop --> callback
                                    |
                          on error: reconnect with backoff (1s -> 30s, +/-12.5% jitter)
                                    |
                        shutdown() -> responds within 100ms, thread joins
```

Monitoring counters (lock-free `AtomicU64`):
- `msg_count` -- messages delivered to callback
- `reconnects` -- successful reconnections
- `drops` -- messages dropped (future: backpressure)

## Socket Tuning

- `TCP_NODELAY` enabled on all connections
- CPU pinning via `core_affinity` (`SocketTuning::core_id`)
- `SO_BUSY_POLL` support on Linux (behind `tuning` feature)

## Features

| Feature | Effect |
|---|---|
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
