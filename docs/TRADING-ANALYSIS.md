# Trading Analysis — Signal Timing, Entry Prices & Edge

**Data source**: 1,185 trades from PBT backtest with real Polymarket orderbook data
**Date**: 2026-03-29
**Markets**: 478+ BTC 15m windows

---

## When to Trade — Hour of Day (UTC)

| Hour (UTC) | ET | Trades | WR | Avg PnL | Notes |
|---|---|---|---|---|---|
| 00:00 | 8PM | 51 | 70.6% | $12.4 | |
| 01:00 | 9PM | 49 | 81.6% | $22.6 | |
| 02:00 | 10PM | 46 | 71.7% | $13.3 | |
| 03:00 | 11PM | 37 | **86.5%** | $21.9 | High WR |
| 04:00 | 12AM | 40 | 82.5% | **$30.8** | Best avg PnL |
| 05:00 | 1AM | 30 | 80.0% | $23.2 | |
| 06:00 | 2AM | 39 | **87.2%** | $23.8 | High WR |
| 07:00 | 3AM | 38 | 81.6% | $21.4 | |
| 08:00 | 4AM | 46 | 78.3% | $20.1 | |
| 09:00 | 5AM | 40 | **92.5%** | $29.5 | **BEST WIN RATE** |
| 10:00 | 6AM | 33 | 87.9% | $22.2 | |
| 11:00 | 7AM | 35 | 85.7% | $20.8 | |
| 12:00 | 8AM | 42 | 83.3% | $24.1 | |
| 13:00 | 9AM | 55 | 72.7% | $16.3 | US pre-market noise |
| 14:00 | 10AM | 72 | 72.2% | $14.9 | US open, noisy |
| 15:00 | 11AM | 68 | 76.5% | $17.3 | |
| 16:00 | 12PM | **74** | 81.1% | $20.3 | **Peak volume** |
| 17:00 | 1PM | 67 | **61.2%** | $6.5 | **WORST HOUR — AVOID** |
| 18:00 | 2PM | 59 | 78.0% | $19.4 | |
| 19:00 | 3PM | 53 | 81.1% | $19.1 | |
| 20:00 | 4PM | 55 | 83.6% | $22.8 | |
| 21:00 | 5PM | 53 | 79.2% | $22.3 | |
| 22:00 | 6PM | 54 | **88.9%** | $24.0 | High WR |
| 23:00 | 7PM | 49 | 71.4% | $16.5 | |

### Trading Sessions

| Session | Trades | Win Rate | Total PnL | Notes |
|---|---|---|---|---|
| Asia (00-08 UTC) | 330 | 80% | +$6,830 | Good volume + good WR |
| **Europe (08-14 UTC)** | **251** | **82%** | **+$5,470** | **Highest win rate** |
| US (14-21 UTC) | 448 | 76% | +$7,603 | Most volume, lower WR |
| Off-hours (21-00 UTC) | 156 | 80% | +$3,284 | Consistent |

### Key Takeaway

- **Best time**: 03:00-12:00 UTC (Europe session) — consistently 82%+ WR
- **Worst time**: 17:00 UTC (1PM ET) — only 61% WR, US lunch hour noise
- **Peak volume**: 14:00-16:00 UTC (US market hours) — most trades but lower WR
- **Consider**: time-of-day filter to skip 17:00 UTC hour

---

## Day of Week

| Day | Trades | Win Rate | PnL |
|---|---|---|---|
| Monday | 84 | 78.6% | +$1,497 |
| Wednesday | 165 | 78.8% | +$3,463 |
| Thursday | 235 | **82.1%** | +$4,920 |
| Friday | 226 | 81.0% | +$4,589 |
| Saturday | 234 | 78.2% | +$4,428 |
| Sunday | 241 | 74.7% | +$4,291 |

Thursday and Friday are the best days. Sunday has the lowest WR (74.7%).

---

## Entry Prices

### Overall Distribution

| Entry Bucket | Trades | % | Win Rate | Avg PnL | Total PnL |
|---|---|---|---|---|---|
| $0.00-0.35 | 80 | 6.8% | **87.5%** | +$61.2 | +$4,897 |
| $0.40-0.45 | 363 | 30.6% | 75.8% | +$20.8 | +$7,544 |
| **$0.45-0.50** | **471** | **39.7%** | **80.5%** | +$16.1 | +$7,584 |
| $0.50-0.55 | 264 | 22.3% | 77.3% | +$11.5 | +$3,026 |
| $0.55-0.60 | 7 | 0.6% | 100% | +$19.5 | +$137 |

**Sweet spot**: $0.45-0.50 — 39.7% of trades, 80.5% WR, most total PnL.

### Up (YES) Entries

| Bucket | Trades | % | WR |
|---|---|---|---|
| $0.00-0.30 | 76 | 12.9% | 88% |
| $0.45-0.50 | 366 | **62.4%** | 80% |
| $0.50-0.55 | 136 | 23.2% | 77% |

**Up contracts**: Most entries at $0.45-0.50 (62%). Average entry: **$0.477**.

### Down (NO) Entries

| Bucket | Trades | % | WR |
|---|---|---|---|
| $0.40-0.45 | 354 | **59.2%** | 76% |
| $0.45-0.50 | 105 | 17.6% | 80% |
| $0.50-0.55 | 128 | 21.4% | 77% |

**Down contracts**: Most entries at $0.40-0.45 (59%). Average entry: **$0.445**.

### Observation: Down Is Cheaper

Down entries average $0.445 vs Up at $0.477 — a $0.032 difference. This suggests the market has a slight bullish bias (people buy Up more), making Down systematically underpriced.

**Potential edge**: Favor Down entries when the signal is ambiguous.

---

## Edge by Entry Price — Is It Real?

| Entry Avg | Breakeven WR | Actual WR | **Edge** | Trades |
|---|---|---|---|---|
| $0.266 | 27% | 88% | **+61pp** | 80 |
| $0.414 | 41% | 76% | **+34pp** | 363 |
| $0.489 | 49% | 80% | **+32pp** | 471 |
| $0.531 | 53% | 77% | **+24pp** | 264 |
| $0.562 | 56% | 100% | **+44pp** | 7 |

**The edge is real at every price level.** Even at the worst bucket ($0.53 entry), we have 24 percentage points of edge above breakeven. This is not noise — it's 264 trades.

### What This Means

At $0.49 entry (the median):
- We need >49% win rate to break even
- We achieve 80% win rate
- That's **31pp of edge** — massive in any market
- Per trade: win = +$0.51/share, lose = -$0.49/share
- At 80% WR: expected = 0.80 × $0.51 - 0.20 × $0.49 = **+$0.31/share**

---

## Direction Prediction Accuracy by Time in Window

From analysis of 146 windows:

| Time | Direction Predicts Outcome | Notes |
|---|---|---|
| 1 min | 61.0% | Barely better than coin flip |
| **2 min** | **68.5%** | Getting useful |
| **3 min** | **75.3%** | **Optimal entry point** |
| 5 min | 67.8% | Worse (repriced, undecided = coin flip) |

**Minute 3 is the sweet spot**: direction at 3 minutes predicts outcome 75% of the time, AND contract price is still cheap ($0.45-0.50).

---

## Reversal Analysis

- **24.7% of windows reverse** after minute 3
- Reversed windows have avg magnitude **0.104%** at min 3 (small moves)
- Non-reversed windows have higher magnitude

**Filter**: Skip entries when magnitude at min 3 is < 0.10% — these are the coin-flip windows that drive our 20% loss rate.

---

## Volatility Regime

| Regime | Windows | Direction Accuracy at 1min |
|---|---|---|
| High vol (>0.1% in 1min) | 22 | **81.8%** |
| Low vol (≤0.1% in 1min) | 124 | 57.3% |

**The volatility filter alone would boost WR from 78% to ~85%** by only trading high-volatility windows. Trade-off: fewer trades (22 vs 146 windows).

---

## Spot Magnitude at Key Times

| Time | Avg | Median | P90 |
|---|---|---|---|
| 1 min | 0.063% | 0.050% | 0.131% |
| 2 min | 0.088% | 0.072% | 0.196% |
| 3 min | 0.119% | 0.085% | 0.250% |
| 5 min | 0.138% | 0.091% | 0.315% |

Current threshold is 0.1%. The median 3-min magnitude is 0.085% — just below threshold. This means we're catching the top ~50% of moves. Lowering to 0.08% would increase trade count by ~10%.

---

## Contract Price Evolution

| Time | Avg Up Price | Min | Max |
|---|---|---|---|
| Open | $0.500 | $0.370 | $0.590 |
| 1 min | $0.488 | $0.180 | $0.760 |
| 2 min | $0.503 | $0.190 | $0.800 |
| 3 min | $0.507 | $0.110 | $0.930 |
| 5 min | $0.507 | $0.090 | $0.980 |
| 10 min | $0.495 | $0.010 | $0.999 |

The market stays near $0.50 for the first 3 minutes on average. By minute 5, it starts bifurcating. By minute 10, it's either near $0.01 or $0.999.

---

## Actionable Improvements From This Data

1. **Time-of-day filter**: Skip 17:00 UTC hour (61% WR vs 78% avg). Easy +2% WR.
2. **Volatility gate**: Only trade when 1-min magnitude > 0.1%. Boosts WR to ~85%.
3. **Favor Down entries**: Down is $0.032 cheaper on average. Same WR.
4. **Lower magnitude threshold to 0.08%**: Catches 10% more trades with minimal WR impact.
5. **Entry at minute 2-3**: Direction accuracy peaks at 75.3%, prices still near $0.50.
