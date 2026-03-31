# Strategy Research — New Ideas from Paper Data + External Research

> Date: 2026-03-30 | Based on 57 live paper trades + research

## Paper Trading Analysis (30 min session)

### What our data shows

| Instance | Record | WR | Key Finding |
|----------|--------|-----|-------------|
| ED-tight | 0W/2L | 0% | Never wins — market reverses after early entry |
| ED-loose | 5W/12L | 29% | Loses money — enters too early, caught on reversals |
| MC-tight | 7W/1L | 88% | Best quality — waits for confirmation, rarely wrong |
| MC-loose | 23W/7L | 77% | Best volume — catches most moves, solid WR |

**Core insight: Confirmation > Speed.** Entering early (first 150s) on initial spot direction has a ~30% WR live. Waiting for sustained momentum (120-600s) has 77-88% WR. The market reverses frequently in the first 2 minutes.

### Direction analysis

- When market resolved **Up**: 80% of our trades won
- When market resolved **Down**: only 47% won

Our strategies have an **Up bias** — they perform much better in rising markets. This suggests we need a strategy that specifically handles Down moves better.

### Per-asset

- BTC: 77% WR (best)
- ETH: 71% WR
- SOL: 50% WR (worst)
- XRP: 46% WR (worst)

SOL and XRP are coin-flip territory. Consider disabling them or using tighter params.

---

## New Strategy Ideas

### Strategy 1: Late-Window Sniper (Last 60 seconds)

**Source:** Benjamin-Cup research on last-second dynamics

When a 5m window has 60 seconds left and the current spot direction is strongly established (>0.3% magnitude), the probability of reversal drops dramatically. A contract priced at $0.70+ in the last minute is nearly certain.

**Implementation:**
```
if time_remaining_secs < 60 && spot_magnitude > 0.003:
    # Market is strongly directional with little time to reverse
    direction_ask = ask for the winning side
    if direction_ask < 0.85:  # still room for profit
        enter at direction_ask → payout $1.00 at resolution
```

**Expected edge:** Very high WR (90%+) but small profit per trade ($0.10-$0.30 per share). High volume compensates.

**Why it works:** Random walk theory says reversals need time. With <60 seconds left and strong momentum, the probability of reversal is very low.

### Strategy 2: Volatility Regime Filter

**Source:** Our own backtest data + PERP Framework research

Our data shows ED loses when volatility is low (small spot moves that reverse). High-volatility windows are much more predictable.

**Implementation:**
```
# Track rolling 1-minute volatility (std dev of spot returns)
if rolling_1min_volatility > threshold:
    # High vol regime — momentum strategies work
    allow MomentumConfirmation
else:
    # Low vol regime — skip or use mean-reversion
    skip trading or use VolatilityBreakout strategy
```

**Config:**
```toml
[[bot.strategies]]
type = "volatility_gated_momentum"
label = "VGM"
min_1min_volatility = 0.002  # skip if vol too low
```

### Strategy 3: Cross-Timeframe Confirmation

**Source:** Target trader analysis (0xe1D6 stacks across 5m/15m/4h)

When BTC is trending Up on the 15m timeframe AND the 5m window is also Up, the 5m trade has much higher WR. Use the 15m trend as a filter for 5m entries.

**Implementation:**
```
# For 5m window:
if ema_15m_trend == direction AND ema_5m_trend == direction:
    # Both timeframes agree — high confidence
    enter with full size
else if only 5m agrees:
    # Lower confidence — reduce size or skip
    enter with half size or skip
```

**Why it works:** When multiple timeframes align, the move is more likely structural (real) vs noise. Our 15m is 100% WR when it first started (before reversals), meaning 15m trends are more reliable.

### Strategy 4: Orderbook Imbalance Entry

**Source:** Quant Playbook + our L2 orderbook data (now flowing)

When bid depth >> ask depth for the Up token, there's buy pressure building. Enter before the price moves.

**Implementation:**
```
imbalance = (bid_depth_5levels - ask_depth_5levels) / total_depth
if imbalance > 0.30:  # strong buy pressure
    enter Up
elif imbalance < -0.30:  # strong sell pressure
    enter Down
```

**We already compute `orderbook_imbalance` in MarketState** — just need a strategy that uses it.

### Strategy 5: Mean Reversion (Counter-Trend)

**Source:** 0xIcaruss "Convergence Fade" strategy

When a 5m window has moved too far too fast (e.g., Up at $0.85 with 3 minutes left), it often reverts. Buy the cheap opposite side.

**Implementation:**
```
if time_elapsed_secs > 180 && direction_ask > 0.80:
    # Market has overshot — buy the opposite side cheap
    opposite_ask = opposite side price  # e.g., $0.20
    if opposite_ask < 0.25:
        enter opposite side → payout $1.00 if reversal
```

**Risk:** Low WR (maybe 20-30%) but huge payoff when it hits (4-5x return). Best as a small hedge alongside momentum positions.

### Strategy 6: End-Cycle Accumulation (Gabagool22 Style)

**Source:** Gabagool22 analysis ($788K profit, 99.5% WR)

Instead of single-shot entries, accumulate both sides throughout the window to capture spread. Market-making strategy.

**Implementation:**
```
# Throughout the window, continuously:
if spread > 0.04:  # wide enough for profit
    buy Up at bid+0.01 AND Down at bid+0.01
    # When window resolves, one side pays $1.00
    # Combined cost < $1.00 = guaranteed profit
```

**Requires:** Maker mode (limit orders). Can't do with taker. Save for Phase 2.

---

## Strategy Priority (What to implement next)

| Priority | Strategy | Complexity | Expected Edge |
|----------|----------|------------|---------------|
| 1 | **Late-Window Sniper** | Low — new Strategy impl | 90%+ WR, small per-trade |
| 2 | **Volatility Regime Filter** | Low — add vol tracking | Filters out noise trades |
| 3 | **Cross-Timeframe Confirmation** | Medium — needs 15m state in 5m eval | Higher 5m WR |
| 4 | **Orderbook Imbalance** | Low — data already available | New signal source |
| 5 | **Mean Reversion** | Low — new Strategy impl | Low WR but high payoff |
| 6 | **End-Cycle Accumulation** | High — needs maker mode | Gabagool-level returns |

## Immediate Action Items

1. **Disable ED-tight and ED-loose** — they're losing money live. Save the capital for MC.
2. **Implement Late-Window Sniper** — highest confidence new strategy
3. **Add Volatility Regime Filter** — prevent noise trades
4. **Track 15m trend for 5m decisions** — cross-timeframe confirmation

---

## Sources

- [5 Strategies Working in 2026 (0xIcaruss)](https://medium.com/@0xicaruss/5-polymarket-strategies-that-are-actually-working-in-2026-with-real-wallet-data-7a56fd547912)
- [Last-Second Dynamics (Benjamin-Cup)](https://medium.com/@benjamin.bigdev/unlocking-edges-in-polymarkets-5-minute-crypto-markets-last-second-dynamics-bot-strategies-and-db8efcb5c196)
- [Quant Playbook for Polymarket (AlgoEdge)](https://algoedgeinsights.beehiiv.com/p/a-polynomial-regression-based-trend-following-strategy-vs-market-backtesting-and-out-of-sample-resul-b140)
- [AI-Augmented Arbitrage (Jung-Hua Liu)](https://medium.com/@gwrx2005/ai-augmented-arbitrage-in-short-duration-prediction-markets-live-trading-analysis-of-polymarkets-8ce1b8c5f362)
- [Gabagool22 Analysis (0xInsider)](https://0xinsider.com/research/gabagool22-polymarket-trader-analysis)
- [Target Trader Deep Dive](docs/TARGET-TRADER-DEEP-DIVE.md)
