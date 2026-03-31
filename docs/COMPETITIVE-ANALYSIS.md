# Competitive Analysis & Strategic Roadmap

> Last updated: 2026-03-30

## Market Landscape (March 2026)

### The Feb 18 Inflection Point

Polymarket silently removed the **500ms taker delay** and introduced **dynamic taker fees**
on all crypto markets. Thousands of simple arb bots died overnight. The game changed from
"fastest taker" to "smartest maker."

Key metrics from public research:

| Metric | Value | Source |
|--------|-------|--------|
| Avg arb opportunity duration | **2.7 seconds** (was 12.3s in 2024) | 0xInsider |
| Arb profit captured by sub-100ms bots | **73%** | 0xInsider |
| Max taker fee at 50% probability | **~1.56%** | Polymarket docs |
| Maker fee | **$0.00** + USDC rebates | Polymarket docs |
| Bot avg profit (880 bots studied) | **$119,156** at 66.4% profitability | 0xInsider |
| Human avg profit (9,702 traders) | **$12,671** at 45.3% profitability | 0xInsider |
| 5-min BTC binary live win rate | **25–27%** (≈ random walk) | Jung-Hua Liu study |
| Breakeven win rate at 50c contract | **~53%** (after fees) | Jung-Hua Liu study |

### Dynamic Fee Curve

Polymarket's fee formula peaks at 50% probability and drops toward zero at the extremes:

```
Fee at 50c → ~1.56%
Fee at 30c → ~0.84%
Fee at 10c → ~0.18%
Fee at 90c → ~0.18%
```

This means:
- **Taker arb at 50c is structurally unprofitable** unless edge exceeds 1.56%
- **Maker mode pays you** to provide liquidity where fees are highest
- Entry at price extremes (<$0.30 or >$0.70) faces minimal fee drag
- The fee curve subsidizes rebate-optimized makers and punishes uncertain takers

### What Top Bots Actually Do

From on-chain analysis of profitable wallets (AgentBets $150K case study, PBot1 analysis):

1. **Maker-first execution** — post limit orders, earn rebates, zero fees
2. **Full L2 orderbook** — local reconstruction from `price_change` WS events
3. **Multi-factor signals** — spot momentum + orderbook imbalance + trend filter
4. **Cross-venue arbitrage** — Polymarket ↔ Kalshi price divergence
5. **Smart timing** — don't rush; wait for optimal orderbook conditions
6. **Position management** — early exit on reversal, partial profit-taking

---

## Our Current Position

### What We Have

- Rust async architecture (tokio) — solid foundation for low latency
- Binance + OKX dual spot feeds with dedup (OracleRouter)
- Real-time PM WebSocket with LatestPrices cache
- Two backtested strategies (EarlyDirectional 81.4% WR, MomentumConfirmation)
- Risk manager with correlation guard, exposure limits, kill switch
- Paper trading live with PBT-compatible recording
- 351 unit tests + 17 integration tests

### What We're Missing

| Gap | Impact | Priority |
|-----|--------|----------|
| **Taker-only execution** | Paying ~1.56% fees on every trade | CRITICAL |
| **Top-of-book only** (best_bid_ask) | No depth visibility, poor slippage estimate | HIGH |
| **No trend filter** | 5m trades approximate random walk without it | HIGH |
| **No fee-aware entry** | May enter negative-EV trades after fees | HIGH |
| **Shared mutable state** (Arc<Mutex>) | Mutex contention, poison risk | MEDIUM |
| **No cross-venue data** (Kalshi) | Missing risk-free arb opportunities | MEDIUM |
| **Hold-to-expiry only** | No early exit, no profit-taking | MEDIUM |

---

## Strategic Improvements (Ranked by ROI)

### Tier 1 — Eliminate Fee Drag (Week 1)

#### 1A. Switch to Maker Mode

The single highest-impact change. Every trade currently loses ~1.56% to taker fees.
As a maker we pay zero and earn rebates.

**Requirements:**
- CLOB API integration (`rs-clob-client` Rust crate exists on GitHub)
- EIP-712 order signing (Polygon/Ethereum typed data signatures)
- GTC limit order placement at our target price
- Cancel/replace cycle when market moves (<100ms target)
- `feeRateBps` field in order signatures (new requirement post-Feb 18)

**Expected impact:** +1.5–3% per trade. At current volumes this roughly doubles net edge.

#### 1B. Fee-Aware Strategy Filter

Before entering, compute effective taker fee at the contract price.
Subtract from expected edge. Skip if net edge < `min_edge`.

```
effective_fee = fee_curve(contract_price)
net_edge = raw_edge - effective_fee
if net_edge < config.min_edge { skip }
```

Our EarlyDirectional enters at max $0.53 — right in the fee danger zone.
Either shift entry to extremes or go maker to avoid fees entirely.

### Tier 2 — Better Signals (Week 2)

#### 2A. Higher-Timeframe Trend Filter

Jung-Hua Liu's study showed adding a 10-min trend filter reduced capital loss from
93% to 13%. The filter is simple:

```
trend = EMA(spot, 10min) vs EMA(spot, 30min)
if trend == flat/choppy → skip trade
if trend != signal_direction → skip trade
```

This is especially critical for 5m windows where signal-to-noise is low.
Our 15m backtest (81.4% WR) already benefits from more signal vs noise.

#### 2B. Local L2 Orderbook Reconstruction

Replace `best_bid_ask` with full depth from `price_change` WebSocket events.

```
Subscribe to CLOB WS → market channel
Process price_change events → BTreeMap<Price, Size> per side
Calculate: real slippage, orderbook imbalance, depth at each level
```

Orderbook imbalance (bid depth vs ask depth) is a strong directional signal
that we currently don't use at all. A `BTreeMap` gives O(log n) inserts with
deterministic iteration order — ideal for checksum validation and level walking.

#### 2C. Smart Entry Timing

Instead of immediately crossing the spread when a signal fires:
1. Monitor orderbook for N seconds (configurable, e.g. 5–10s)
2. Wait for ask-side depth to thin (fewer contracts competing)
3. Or wait for spread to narrow
4. Post a maker limit order at our target price

### Tier 3 — New Revenue Streams (Week 3)

#### 3A. Cross-Venue Arbitrage (Polymarket ↔ Kalshi)

Same BTC/ETH Up/Down contracts exist on Kalshi. When prices diverge:

```
if PM_Up_ask + Kalshi_Down_ask < $1.00:
    buy Up on PM, buy Down on Kalshi
    guaranteed profit = $1.00 - combined_cost
```

This is TRUE risk-free arbitrage (both legs executed simultaneously).
Multiple open-source implementations exist. Requires Kalshi API credentials.

#### 3B. Position Exit Intelligence

Currently we hold every position to expiry. Better approaches:
- **Take profit**: if contract moves 20%+ in our favor, sell half
- **Cut losses**: if spot reverses significantly, exit early
- **Hedge lock**: buy opposite side to lock in profit (needs two-leg execution)

### Tier 4 — Architecture (Week 4)

#### 4A. Actor-Based Ingestion (No Mutex Hot Path)

Replace `Arc<Mutex<OrderbookTracker>>` with a channel-based actor:

```
[WS Task] --typed events--> [Ingestion Actor] --MarketState--> [Strategy Loop]
```

Single task owns all mutable state. No locks on the critical path.
This is the pattern from the Kraken-RS ingestion system and NautilusTrader.

Benefits:
- Zero mutex contention in hot path
- No poison risk
- Deterministic message ordering
- Simpler recovery logic on disconnect

#### 4B. CPU Pinning & Buffer Reuse

For sub-10ms latency:
- Pin ingestion task to dedicated CPU core
- Pre-allocate parse buffers, reuse across messages
- Use `simd-json` for in-place parsing (we have the dep, just not using it)
- Consider `io_uring` for socket I/O (Linux only)

---

## 5-Minute vs 15-Minute Window Analysis

| Aspect | 5-Minute | 15-Minute |
|--------|----------|-----------|
| Signal quality | Low (random walk) | Higher (momentum persists) |
| Backtest WR | ~55–60% | **81.4%** |
| Live WR (Liu study) | 25–27% | Not studied but higher |
| Competition | Intense (many bots) | Less saturated |
| Fee impact | Higher (more trades) | Lower (fewer trades) |
| Our edge | Weak without trend filter | Strong with current params |

**Recommendation:** Focus on 15-minute as primary, use 5-minute only with trend filter enabled.

---

## Target Performance Model

### Current (Taker, Top-of-Book)

```
Avg entry:        $0.52
Taker fee:        ~1.5%
Gross edge:       ~4%
Net edge:         ~2.5%
Trades/day:       ~20 (15m windows, 4 assets)
Avg size:         $25
Daily net PnL:    ~$12.50
Monthly:          ~$375
```

### After Tier 1+2 (Maker, L2 Book, Trend Filter)

```
Avg entry:        $0.50 (better fills as maker)
Maker fee:        $0.00 + rebate
Gross edge:       ~5% (trend filter removes noise)
Net edge:         ~5.5% (rebate adds to edge)
Trades/day:       ~15 (fewer but higher quality)
Avg size:         $50 (more confidence → bigger size)
Daily net PnL:    ~$41
Monthly:          ~$1,230
```

### After Tier 3+4 (Cross-Venue, Exit Logic, Actor Arch)

```
Directional PnL:  ~$41/day
Cross-venue arb:  ~$15/day (conservative)
Better exits:     +20% on directional
Daily net PnL:    ~$64
Monthly:          ~$1,920
```

These are conservative estimates assuming $500 capital. Scale linearly with capital.

---

## Key Research References

- [AI-Augmented Arbitrage in 5-Min BTC Markets](https://medium.com/@gwrx2005/ai-augmented-arbitrage-in-short-duration-prediction-markets-live-trading-analysis-of-polymarkets-8ce1b8c5f362) — Jung-Hua Liu, Mar 2026. Live trading study showing 5m BTC ≈ random walk.
- [How a Polymarket Arb Bot Made $150K](https://agentbets.ai/blog/polymarket-arbitrage-bot-case-study/) — AgentBets, Mar 2026. On-chain analysis of 50K+ trades.
- [Bots vs Humans: 10,582 Traders](https://0xinsider.com/research/bots-vs-humans-polymarket-trading) — 0xInsider, Mar 2026. 880 bots vs 9,700 humans.
- [Polymarket's Fee Curve Is Taxing the Middle](https://liquidityguide.com/blog/polymarket-maker-rebates-all-crypto-markets) — Liquidity Guide, Mar 2026. Deep dive on fee/rebate economics.
- [Deterministic WebSocket Ingestion in Rust](https://dev.to/nihalpandey2302/building-a-deterministic-high-throughput-websocket-ingestion-system-in-rust-38ia) — Dev.to, Feb 2026. Actor-based ingestion architecture.
- [Polymarket Dynamic Fees](https://www.financemagnates.com/cryptocurrency/polymarket-introduces-dynamic-fees-to-curb-latency-arbitrage-in-short-term-crypto-markets/) — FinanceMagnates. Fee structure impact analysis.
- [Polymarket WebSocket Guide](https://agentbets.ai/guides/polymarket-websocket-guide/) — AgentBets, Mar 2026. WS channels and subscription patterns.
- [Polymarket CLOB Rust Client](https://github.com/Polymarket/rs-clob-client) — Official Rust SDK for order placement.
- [PBot1 Bot Analysis](https://agentbets.ai/news/pbot1-polymarket-bot-analysis/) — AgentBets, Mar 2026. Reverse-engineering a live bot's strategy.
- [Polymarket Slippage with L2 Order Books](https://www.polymarketdata.co/blog/polymarket-slippage-l2-order-book-guide) — PolymarketData. Real slippage from historical L2 data.
