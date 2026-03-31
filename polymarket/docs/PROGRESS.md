# Polymarket Trading Bot — Progress

## Status: LIVE TRADING (March 31, 2026)

**Portfolio:** ~$100 USDC on Polymarket
**Today's P&L:** +$7.61 (7.20%)
**Session (this binary):** 4W/0L, +$9.92 combined across strategies

---

## Architecture

- **7 workspace crates**: pm-types, pm-signal, pm-market, pm-oracle, pm-risk, pm-executor, pm-live
- **3 live strategies**: LWS (Late Window Sniper), MC-loose, MC-tight
- **Order execution**: GTC limit orders via Polymarket CLOB SDK (polymarket-client-sdk v0.4)
- **Fill monitoring**: User WebSocket for real-time fill events
- **Resolution**: Oracle polling via CLOB REST API (`GET /markets/{condition_id}`)
- **Price feeds**: Binance + OKX WebSocket (spot), Polymarket WS (contract prices)

## Completed Features

### Core Trading Engine
- [x] Strategy trait system with independent instances (own balance, positions, risk)
- [x] ConcreteStrategyInstance for paper trading
- [x] LiveStrategyInstance wrapping paper with real CLOB execution
- [x] Per-strategy `mode` field (paper/live) in TOML config
- [x] Per-strategy `order_mode` field (fok/gtc) in TOML config

### Strategies
- [x] EarlyDirectional — enters first 3 min on strong opening move
- [x] MomentumConfirmation (loose + tight) — mid-window momentum confirmation
- [x] LateWindowSniper — last 60s entries with high conviction
- [x] MeanReversion — fades overshoots (paper only)
- [x] CompleteSetArb — buy both sides when combined cost < $1 (paper only)

### Live Execution (pm-live crate)
- [x] CLOB client authentication (GnosisSafe proxy wallet, EIP-712)
- [x] FOK market orders with slippage protection
- [x] GTC limit orders (no post_only — fills as taker if crossing, maker if resting)
- [x] OrderManager for pending GTC order tracking + timeout
- [x] User WebSocket client for real-time fill monitoring
- [x] SDK heartbeat for keeping resting orders alive
- [x] Non-blocking async order dispatch (tokio::spawn, no block_in_place)
- [x] Token map cache to avoid mutex lock on hot path

### Safety & Validation
- [x] Double price guard [0.15, 0.85] on live entries
- [x] $1 minimum order size (CLOB requirement)
- [x] 5-share minimum for GTC orders
- [x] Scanner: reject unknown timeframes (no daily→15m misclassification)
- [x] Scanner: slug+endDate validation (catches wrong durations)
- [x] Token map: clear on scan + active window only (no stale entries)
- [x] Window slot dedup per strategy instance

### Resolution & P&L
- [x] Oracle-based position resolution via CLOB API polling
- [x] Condition ID stored at order placement time (not stale token map lookup)
- [x] Accurate WIN/LOSS tracking matching Polymarket's actual oracle
- [x] Rich signal logging (window open price, spot, magnitude, asks, confidence)

### Market Data
- [x] Binance + OKX WebSocket for spot prices
- [x] Polymarket WebSocket for contract bid/ask
- [x] Gamma API scanner for active market discovery
- [x] L2 orderbook reconstruction from "book" WS events
- [x] REST orderbook snapshot fetching for new tokens
- [x] Polymarket taker fee model (crypto: 0.072 rate, 1.80% peak at 50c)

### Infrastructure
- [x] Actor-based ingestion (PmEvent channel, no shared mutex)
- [x] PBT-compatible window recording
- [x] Configurable via TOML (config/live-test.toml)
- [x] run-live.sh launch script
- [x] Git repo at github.com/Milerius/Mantis (polymarket/ subfolder)

## Known Issues / TODO

### Auto-Redemption (not working)
- Post-resolution auto-sell fails ("orderbook does not exist" — book closed)
- CTF direct call fails (tokens in Safe/Proxy wallet, not EOA)
- **Options:**
  1. Relayer API (needs Builder API credentials from Polymarket)
  2. Switch to EOA wallet (direct CTF calls work)
  3. Sell before resolution at $0.95+ (loses $0.01-0.05/share)
- **Current workaround:** Manual redeem in Polymarket UI

### Strategy Accuracy
- MC-loose fires on small magnitude (0.1%) — gets whipsawed in choppy markets
- Consider: higher magnitude threshold, time-of-day filter, trend filter
- Paper backtest showed 89.9% win rate but live is lower due to execution timing

### Fill Rate
- GTC orders without post_only: fill rate improved vs post_only
- Overnight markets have zero liquidity — bot posts but nobody takes
- Active hours (13:00-21:00 UTC) have much better fill rate

### SDK Bugs
- Heartbeat bug (Issue #239): `heartbeat_token` on Client not ClientInner
  - Cloning client (during order build) can kill heartbeat task
  - Currently mitigated: orders survive long enough in practice
- `market_resolved` WS event never sent (Issue #226)
  - Mitigated: oracle polling via REST API

## Performance Profile

| Step | Latency |
|------|---------|
| Signal evaluation | 10-50µs |
| Token map lookup (cached) | <5µs |
| GTC order dispatch (async) | <1ms (non-blocking) |
| CLOB API round-trip | 50-200ms (background) |
| Oracle poll (per position) | 50-200ms |
| Chainlink resolution delay | 2-5 min after window close |

## Config (live-test.toml)

- Assets: BTC, ETH, SOL (XRP disabled)
- LWS: GTC, 55s timeout, $0.85 max entry, 0.2% min magnitude
- MC-loose: GTC, 120s timeout, $0.65 max entry, 0.1% min magnitude
- MC-tight: GTC, 120s timeout, $0.72 max entry, 0.3% min magnitude
- Balance: $40 per strategy
- Kelly fraction: 0.20-0.25
- Max daily loss: $15 per strategy
