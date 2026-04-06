# mantis-strategy

Venue-agnostic strategy runtime primitives for the Mantis low-latency financial SDK.

## Overview

This crate provides the building blocks for implementing trading strategies. All types are `no_std` compatible, use fixed-size arrays, and allocate nothing on the heap.

Prediction-market-specific types (YES/NO positions, settlement PnL, merge operations) belong in `mantis-prediction`, not here.

## Components

| Type | Purpose |
|------|---------|
| `Strategy` | Event-driven trait — `on_event(&mut self, event, intents) -> usize` |
| `OrderIntent` | Post/Cancel/Amend intent, `repr(C)`, `Copy`, flows through SPSC rings |
| `Position` | Signed inventory (`SignedLots`), VWAP entry, realized/unrealized PnL |
| `OrderTracker` | Fixed-size `[Option<TrackedOrder>; 64]` order state machine |
| `QueueEstimator` | L2 probabilistic queue position model (PowerProbQueueFunc) |
| `ExposureView` | Composes position + open orders for worst-case risk calculation |
| `RiskLimits` | Per-strategy risk configuration (position, capital, rate limits) |
| `StrategyContext` | Optional helper bundling engine + queue + orders + risk + positions |

## Strategy Trait

```rust
use mantis_strategy::{Strategy, OrderIntent, MAX_INTENTS_PER_TICK};
use mantis_events::HotEvent;

struct MyStrategy { /* ... */ }

impl Strategy for MyStrategy {
    const STRATEGY_ID: u8 = 0;
    const NAME: &'static str = "my-strategy";

    fn on_event(
        &mut self,
        event: &HotEvent,
        intents: &mut [OrderIntent; MAX_INTENTS_PER_TICK],
    ) -> usize {
        // Process event, return number of intents written
        0
    }
}
```

## Design Decisions

- **No generics on trait** — `OrderBook` type and `MAX` instruments are implementation details inside each concrete strategy, not on the trait surface
- **Associated consts** — `STRATEGY_ID` and `NAME` are compile-time, not runtime methods
- **Enum dispatch** — bot binaries use enum dispatch (not `dyn`) for zero vtable overhead
- **`Lots` vs `SignedLots`** — unsigned for order sizes, signed for position inventory
- **Fixed arrays** — `[Option<T>; 64]` everywhere, no HashMap, no Vec, no heap
- **Replay-friendly** — strategy is a pure event→intent state machine. Same tape = same intents

## Queue Position Model

The `QueueEstimator` uses a probabilistic model based on the `PowerProbQueueFunc` from [hftbacktest](https://github.com/nkaz001/hftbacktest):

- Cancels are biased toward the back of the queue (parameter `n=2-3`)
- Fill probability estimated via Poisson model: `P[Poisson(take_rate * time) >= ahead_qty + order_qty]`
- Take rate tracked per instrument per side via EWMA
- Calibrate `n` from live fill data

## License

Apache-2.0
