# Polymarket Trading Bot — Progress

**Last updated**: 2026-03-29 17:10 UTC
**Branch**: `feature/polymarket-bot`

---

## Current State: Paper Trading Live + Backtest Running

The bot is operational in paper trading mode with all 3 WebSocket feeds connected (Binance, OKX, Polymarket). Historical backtest running continuously against real PolyBackTest orderbook data.

---

## Backtest Results (Real Orderbook Data)

| Metric | Value |
|---|---|
| **Markets tested** | 408 (growing — PBT download at 408/3,087) |
| **Total trades** | 749 |
| **Win rate** | 78.0% |
| **P&L** | +$19,834 (from $500 initial) |
| **Profit factor** | 5.81 |
| **Sharpe ratio** | 147.8 |
| **Max drawdown** | -$100 |

### Per-Strategy Breakdown

| Strategy | Trades | Win Rate | P&L | Status |
|---|---|---|---|---|
| **EarlyDirectional** | 708 | 77.8% | +$16,124 | Primary edge |
| **MomentumConfirmation** | 41 | 80.5% | +$3,710 | Works on sustained moves |
| **CompleteSetArb** | 0 | — | $0 | Market too efficient for 15m arb |
| **HedgeLock** | 0 | — | $0 | Combined prices stay ~$1.01 |

### Key Findings

- **EarlyDirectional is the core edge**: Enter in first 3 minutes of a window when spot has moved >0.1%, contract price still <$0.58
- **Win rate stable at ~78-81%** across 400+ markets — statistically significant
- **Avg entry price ~$0.50**: At this level, need >50% WR to profit. We have 78%.
- **Profit factor 5.81**: For every $1 lost, $5.81 gained
- **Momentum improving**: Was 50% WR at 14 trades, now 80.5% at 41 trades

---

## Paper Trading Status

**State**: Running live, waiting for BTC volatility

| Component | Status |
|---|---|
| Binance WebSocket | Connected, ~10 ticks/sec |
| OKX WebSocket | Connected |
| Polymarket WebSocket | Connected, 64 tokens subscribed, orderbook streaming |
| Market Scanner | 32 active markets (BTC+ETH, 5m+15m) |
| Window Tracking | BTC+ETH across 5m/15m/1h/4h |
| Strategy Engine | 4 strategies evaluating every tick |
| Paper Executor | Ready, $500 initial balance |
| Live Recorder | Recording ticks + orderbook to compressed JSONL |

**No signals fired yet** — BTC is flat (moved 0.03% in current window, threshold is 0.1%). The bot correctly waits for real volatility.

---

## Infrastructure

### Crates (7 total, 370+ tests)

| Crate | Purpose | Tests | Status |
|---|---|---|---|
| **pm-types** | Newtypes, enums, config | 62 | Complete |
| **pm-signal** | 4 strategies + engine | 57 | Complete |
| **pm-oracle** | Binance/OKX WS, PBT, data pipeline | 109 | Complete |
| **pm-market** | Polymarket scanner + WS orderbook | 30+ | Complete |
| **pm-risk** | Kelly sizing, exposure, kill switch | 17 | Complete |
| **pm-executor** | Backtest + paper + (future) live | 10+ | Backtest + paper done |
| **pm-bookkeeper** | Trade log, summary, export, recorder | 16+ | Complete |

### CLI Commands

```bash
polymarket download        # Binance + Polymarket historical data
polymarket calibrate       # Build fair_value models
polymarket backtest        # Run backtest with model prices
polymarket sweep           # Parameter sweep (1,944 combos in 7.4s)
polymarket pbt-download    # Download PolyBackTest orderbook snapshots
polymarket pbt-backtest    # Backtest with real PBT orderbook data
polymarket paper           # Live paper trading with all WebSocket feeds
```

### Performance

| Benchmark | Result |
|---|---|
| Signal evaluate (lookup) | 1.7 ns |
| Signal evaluate (logistic) | 4.3 ns |
| Signal evaluate (full engine) | 6-18 ns |
| Sweep: 1,944 backtests × 691K ticks | 7.4s (rayon parallel) |
| Tick-to-order latency | ~5-11ms (estimated) |

---

## Data

### PolyBackTest (real orderbook snapshots)

- **Downloaded**: 408/3,087 BTC 15m markets (download running, ~3h remaining)
- **Format**: Compressed JSONL with full orderbook depth
- **Source**: PolyBackTest API (Pro plan, 8 snapshots/sec per market)
- **Total size**: ~1.2 GB so far

### Binance Historical (1s candles)

- **BTC**: 51 days cached
- **ETH**: 90 days cached (full 3 months)

### Live Recorded Data

- Paper trading records ticks + orderbook to `data/live/` for future replay
- ~35 MB/day compressed estimated

---

## Target Trader Analysis

**Wallet**: `0xe1D6b51521Bd4365769199f392F9818661BD907`
**Stats**: $93,671 PnL, $3,122/day, 0.68% edge per dollar

### Their Strategy (from 30 live positions)

- Trades BTC + ETH + SOL + XRP simultaneously
- **47% of trades on 5m windows** (we don't trade these yet)
- Multi-timeframe stacking: 5m + 15m + 4h + daily
- Entry prices: $0.38-0.83 (early + momentum + late)
- Switches Up/Down every few windows following spot
- Position sizes: $612-4,694 per trade

### Speed Requirement

- 23% of entries in first 30-90s → need 10-30s reaction time
- 43% in first 1-3 min → plenty of time
- 33% after 3 min → speed irrelevant, accuracy matters
- **Our bot is 1000x faster than needed** — edge is signal quality, not speed

---

## Improvement Roadmap

See `docs/IMPROVEMENTS.md` for full details.

| Priority | Improvement | Expected Impact |
|---|---|---|
| **P0** | Add 5m window support | +47% more trades (target trader's main timeframe) |
| **P1** | Raise momentum threshold to $0.80 | Catch more momentum entries |
| **P2** | Enable SOL + XRP | More assets = more opportunities |
| **P3** | Add 4h + daily timeframes | Multi-timeframe stacking |
| **P5** | Volatility regime filter | +5% WR boost (81.8% vs 57.3%) |
| **P6** | Anti-reversal magnitude filter | Fewer bad trades |
| **P7** | Scale position sizes | $25 → $500+ per trade |
| **P8** | Orderbook imbalance signal | New alpha source |

---

## Phase Completion

| Phase | Status | Key Deliverable |
|---|---|---|
| **Phase 1** | Complete | Workspace, types, signal engine, backtest |
| **Phase 1.5** | Complete | 4-strategy engine, realistic backtest with PBT data |
| **Phase 2** | Running | Paper trading with live WS feeds + data recording |
| **Phase 3** | Not started | Live execution (CLOB client, EIP-712 signing) |
| **Phase 4** | Not started | OpenClaw monitoring agent |

---

## Git History (key commits)

```
1221ab5 fix: paper trading tick channel + logging
56eb6d3 docs: target trader analysis + improvement roadmap (P0-P9)
95071c2 feat: Phase 2 paper trading + live data recording
bfd067e feat: Polymarket WebSocket live orderbook + paper trading fully wired
508be8a feat: PolyBackTest integration with real orderbook data
ce913d2 results: 85 trades, 68.2% WR, +$912 on 34 markets
0d52403 perf: 5.6x sweep speedup — rayon + zero-alloc hot path
f942cfc docs: V2 strategy redesign spec + Phase 1.5 plan
f56f6a4 fix(backtest): separate engine min_edge from market price simulation offset
```
