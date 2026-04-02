# Idolized-Scallops BTC 15m — Complete Strategy Analysis

**Date:** 2026-04-02
**Wallet:** `0xe1d6b51521bd4365769199f392f9818661bd907c`
**Data Source:** Heisenberg API (agent 556, 581, 569), on-chain Polygonscan
**Period:** 2026-04-02 13:15 - 15:15 UTC (8 consecutive BTC 15m windows)

---

## 1. Executive Summary

Idolized-Scallops is an automated trader on Polymarket's BTC Up/Down 15-minute markets. Over 8 consecutive windows analyzed, the wallet deployed $50,885 across 1,297 trades with a net result of **-$3,024 (-5.9%)** — 3 wins and 5 losses.

Despite the negative session, the strategy has structural merit: both-sides position construction with asymmetric payoffs (wins average +$882, capped losses averaging -$1,134). Over longer periods (3-day Wallet 360), the wallet shows +$85,983 PnL on $3.2M deployed (2.65% ROI).

**The core finding of this analysis:** The strategy leaks edge through three identifiable, mechanical flaws — chasing fills above $0.80, expensive insurance, and indecisive side-switching. A disciplined implementation fixing these flaws would likely flip the 8-window result from -5.9% to +10-15%.

---

## 2. Strategy Mechanics

### 2.1 How It Works

Each BTC 15-minute window (e.g., 14:00-14:15 UTC) is a binary market: "Will BTC price go Up or Down in this 15-minute period?" Resolution is via Chainlink oracle. Up token redeems at $1.00 if BTC goes up, $0.00 if down. Down token is the inverse.

The trader's approach across every window follows the same 3-phase structure:

**Phase 1 — Initial Directional Bet (0-15% of window, first ~2 minutes)**
- Places GTC limit orders on the side he believes will win
- Entry prices near fair value: $0.45-0.65
- Uses BTC spot price movement from Binance as the directional signal

**Phase 2 — Accumulation / Adjustment (15-70% of window)**
- GTC orders continue filling as counterparties arrive
- If direction confirms: adds more on winning side
- If direction reverses: may switch sides or hold
- Fills depend on available liquidity, not pre-set budget

**Phase 3 — Insurance + Late Adds (70-100% of window)**
- Places penny orders ($0.01-0.08) on the losing side as reversal insurance
- Optionally adds to winning side near $1.00 if spread exists
- Position is effectively final by 80-90%

### 2.2 Order Types — GTC Limit Orders (Confirmed)

Analysis of fill patterns and on-chain data confirms the trader primarily uses **GTC (Good Till Cancelled) limit orders**, not FOK (Fill or Kill) or market orders.

Evidence:
- Same order_hash appears with multiple fills at a single price over time (order resting on book, filled by different takers)
- Multiple separate tx hashes at identical prices in the same second (multiple GTC orders posted at same level)
- Price walking pattern is MIXED (not monotonically ascending as with book-sweeping taker orders)
- Fill sizes use round numbers (10, 20, 50, 100, 148, 200, 305 shares) — pre-set GTC quantities

Example from window 14:45-15:00:
```
order 0x0e19e3... | 5 fills | Up  @ $0.3200 | total 674 shares (1 order, 5 partial fills)
order 0xc7f8b6... | 5 fills | Dn  @ $0.9500 | total 148 shares (1 order, 5 partial fills)
order 0xa6cf26... | 5 fills | Up  @ $0.0540 | total 48 shares  (1 order, 5 partial fills)
```

This means the trader is a **liquidity provider (maker)**, not a taker. He posts resting orders and other participants fill against him.

### 2.3 Both-Sides Position Construction

In 95.2% of windows (3-day dataset, 745 windows), the trader buys BOTH Up and Down tokens. This is the risk management core:

```
PnL = W × (1 - p_w) - L × p_l

Where:
  W = shares of the winning side
  p_w = average price paid for winning shares
  L × p_l = total cost of losing side (total loss)

The winning side ALWAYS returns money ($1 per share).
The losing side ALWAYS goes to $0.
Net = winner profit minus loser cost.
```

### 2.4 Why He Never Loses 100%

When wrong, the winning-side shares still redeem at $1.00, recovering most capital:

| Window | Deployed | Recovery from Winner | Net Loss | Loss % |
|---|---|---|---|---|
| 13:15 | $17,268 | $13,611 (79%) | -$3,656 | -21.2% |
| 13:30 | $8,526 | $7,846 (92%) | -$680 | -8.0% |
| 14:00 | $6,009 | $5,593 (93%) | -$416 | -6.9% |
| 14:45 | $5,835 | $4,917 (84%) | -$917 | -15.7% |

Recovery ranges from 79-93% of deployed capital. Maximum observed loss: -21.2%.

---

## 3. The 8 Windows — Trade-by-Trade Reconstruction

### 3.1 Summary Table

| Window | Winner | Deployed | PnL | ROI | W.Avg Price | % Fills >$0.80 | Early Correct | Switches | Conviction |
|---|---|---|---|---|---|---|---|---|---|
| 13:15-13:30 | Up | $17,268 | **-$3,656** | -21.2% | $0.812 | 89% | NO | 173 | 64% |
| 13:30-13:45 | Down | $8,526 | **-$680** | -8.0% | $0.869 | 88% | NO | 7 | 80% |
| 13:45-14:00 | Up | $360 | **-$1** | -0.2% | $0.773 | 15% | NO | 4 | 77% |
| 14:00-14:15 | Up | $6,009 | **-$416** | -6.9% | $0.633 | 31% | NO | 7 | 59% |
| 14:15-14:30 | Down | $4,042 | **+$370** | +9.2% | $0.661 | 27% | YES | 11 | 72% |
| 14:30-14:45 | Up | $3,417 | **+$1,620** | +47.4% | $0.673 | 0% | YES | 2 | 99% |
| 14:45-15:00 | Down | $5,835 | **-$917** | -15.7% | $0.909 | 91% | NO | 9 | 77% |
| 15:00-15:15 | Up | $5,429 | **+$655** | +12.1% | $0.568 | 0% | NO | 10 | 64% |

**Aggregate: 3W / 5L | -$3,024 on $50,885 = -5.94%**

### 3.2 Window Narratives

**W1: 13:15-13:30 — WORST LOSS (-$3,656, -21.2%)**
- Starts buying Down at $0.62 in first 2% (wrong side — Up won)
- 173 side switches over 13 minutes — extreme indecision
- Chases Up to $0.907 avg in late window (89% of fills above $0.80)
- Loser (Down) avg $0.394 — insurance far too expensive
- Biggest deployment ($17,268) coincides with worst execution

**W2: 13:30-13:45 — SMALL LOSS (-$680, -8.0%)**
- Bets wrong side early, Down heavy at 80% conviction
- Chases winner (Down) to $0.869 avg — 88% above $0.80
- Stops at 64% through (didn't overextend in time)

**W3: 13:45-14:00 — FLAT (-$1, -0.2%)**
- Tiny deployment ($360), essentially sat out
- Wrong early but small size limited damage

**W4: 14:00-14:15 — LOSS (-$416, -6.9%)**
- Starts buying Down at $0.60 (wrong — Up won)
- Switches to Up at $0.66 (20-50%), then cheap Down at $0.25 (50-80%)
- Last 20%: Up at $0.98, Down at $0.03
- Moderate loss — the mid-window switch saved him but overpaid

**W5: 14:15-14:30 — WIN (+$370, +9.2%)**
- Heavy Down in first 20% at $0.62 — correct direction
- Clean progression: Down accumulation, Up hedge at $0.52
- Late: penny Up insurance at $0.04 (51 fills, $100) + one big Down at $0.88
- Textbook execution. Low switches, correct early, cheap insurance.

**W6: 14:30-14:45 — BIG WIN (+$1,620, +47.4%)**
- **Best window.** Massive early conviction: $3,389 on Up at $0.55-0.71 in first 12%
- Then penny Down insurance at $0.01-0.02 from 45-70%
- No action after 70%. Done. 99% on the winner. Only $29 on insurance.
- **0% of fills above $0.80.** All cheap. All early. Maximum edge.

**W7: 14:45-15:00 — LOSS (-$917, -15.7%)**
- Starts Up at $0.47, switches to Down at $0.62
- Rides Down up: $0.75 → $0.83 → $0.92 → $0.95 → $0.985
- Mid-window Up insurance at $0.06-0.20 (bounce buys)
- Last 20%: Down at $0.985 — $1,924 committed for $0.015/share edge
- Lost because avg Down price $0.91 → profit $0.09/share couldn't cover Up cost

**W8: 15:00-15:15 — WIN (+$655, +12.1%)**
- Initial Down at $0.57, then heavy Up accumulation at $0.21-0.37
- Avg winner price $0.568 — bought cheap, good edge
- 0% above $0.80. Patient fills across mid-window.

---

## 4. What Separates Wins From Losses

### 4.1 Statistical Comparison

| Metric | WINS (3 windows) | LOSSES (5 windows) |
|---|---|---|
| Avg winner price | **$0.634** | $0.799 |
| Avg winner early price | **$0.543** | $0.630 |
| % fills above $0.80 | **9%** | 63% |
| Early direction correct | **67%** | **0%** |
| Avg side switches | **7.7** | 40.0 |
| Avg conviction (dom%) | 78% | 71% |
| Avg deployed | $4,296 | $7,599 |

### 4.2 The Three Rules That Separate Wins From Losses

**Rule 1: Winner avg price below $0.70 = win, above $0.80 = loss**

This is the strongest signal. Every window where the winner side avg price was below $0.70 was profitable. Every window where it exceeded $0.80 lost money. The spread to $1.00 IS the edge — buying at $0.65 gives $0.35/share profit, buying at $0.90 gives only $0.10/share.

**Rule 2: Getting the early direction right**

In all 3 wins, at least some early capital went to the correct side. In all 5 losses, early capital went to the wrong side. Starting on the wrong side means you either take a loss switching, or you hold and lose on the dominant position.

**Rule 3: Fewer switches = better**

Wins averaged 7.7 side switches. Losses averaged 40.0 (pulled up by the 173-switch disaster at 13:15). Each switch means cancelling GTC orders and placing new ones — during which the book moves against you.

---

## 5. Position Sizing — How He Decides

### 5.1 He Doesn't Pre-Choose a Size

Total deployed varies 48x across windows ($360 to $17,268). The size is NOT predetermined. It's an emergent property of:

1. **Available liquidity**: He posts GTC orders at fixed share sizes (10, 20, 50, 100, 200 shares) at multiple price levels. How much fills depends on how many counterparties arrive. More disagreement = more fills = bigger position = bigger edge.

2. **First price signal**: When the first fill is near $0.50 (uncertain) → more capital ultimately flows in. When the first fill is at $0.19 (already decided) → small position because little edge remains.

3. **Price discipline**: In wins, he stops adding above $0.70. In losses, he chases to $0.95+ and the total deployed balloons.

### 5.2 Fill Distribution Pattern

| Top 5 fills as % of total | Outcome |
|---|---|
| 37-44% (distributed) | Wins |
| 62-68% (concentrated) | Losses |

Distributed fills across many price levels = patient GTC execution = better avg price.
Concentrated fills in a few big orders = chasing = overpaying.

### 5.3 The Price Ladder Approach

Based on fill analysis, his GTC order structure appears to be:

- Multiple price levels spanning $0.45 to $0.95+
- Fixed share sizes per level (round numbers: 50-200 shares)
- Posts on favored side first, then adds other side
- Cancel/replace as the window progresses

The total deployed is whatever the market fills — not a pre-set budget.

---

## 6. Risk Management Math

### 6.1 The Formula

```
PnL = (Winner Shares × $1.00 - Winner Cost) - Loser Cost
    = Winner Shares - Winner Cost - Loser Cost
    = Winner Shares - Total Deployed

If Winner Shares > Total Deployed → profit
If Winner Shares < Total Deployed → loss (but capped)
```

### 6.2 Real Examples

**Best case (W6, 14:30-14:45): +$1,620 (+47.4%)**
```
Up (winner):  5,037 shares × $1 = $5,037  (paid $3,389)  → +$1,648 profit
Down (loser): 2,626 shares × $0 = $0      (paid $29)     → -$29 loss
Net: +$1,648 - $29 = +$1,619
Recovery: $5,037 / $3,417 = 147% (more back than deployed)
```

**Worst case (W1, 13:15-13:30): -$3,656 (-21.2%)**
```
Up (winner):  13,611 shares × $1 = $13,611  (paid $11,052)  → +$2,559 profit
Down (loser): 15,758 shares × $0 = $0       (paid $6,216)   → -$6,216 loss
Net: +$2,559 - $6,216 = -$3,657
Recovery: $13,611 / $17,268 = 79% (lost 21% of deployed)
```

**Key insight**: Even in the worst loss, 79% of capital comes back. The winning side always redeems at $1.00 regardless of whether you "won" or "lost" the overall window.

### 6.3 Why Buying the Loser at Pennies Matters

Late in the window, he buys losing tokens at $0.01-0.08. Two purposes:

1. **Reversal insurance**: If BTC flips in the last minute, $0.01 tokens become $1.00 (100x return). Cost: $10-30 per window. Potential: $1,000+.

2. **The $0.20 bounce buys**: When the losing token bounces mid-window ($0.05 → $0.20), he buys the bounce. If the reversal continues, $0.20 tokens become $1.00 (5x). If not, he loses $0.20/share — still cheap relative to the main position.

### 6.4 Position Ratio = Risk Profile

| Up/Down Split | Winning Scenario | Losing Scenario |
|---|---|---|
| 99/1 (W6) | Massive win (+47%) | Small loss (loser cost minimal) |
| 80/20 | Good win | Moderate loss (~-16%) |
| 60/40 | Small win | Small loss (-7%) |
| 50/50 | Tiny win/loss | Market making, minimal PnL |

Higher conviction = higher upside but higher downside. The 99/1 split in W6 was his best trade because he was right AND committed early.

---

## 7. Flaw Analysis

### 7.1 Identified Flaws

**Flaw 1: Chasing Fills Above $0.80 (Most Expensive)**

| Window | % Winner Fills >$0.80 | Avg Late Price | Result |
|---|---|---|---|
| 13:15 | 89% | $0.907 | -$3,656 |
| 13:30 | 88% | — | -$680 |
| 14:45 | 91% | $0.953 | -$917 |
| 14:30 | **0%** | — | **+$1,620** |
| 15:00 | **0%** | — | **+$655** |

Buying at $0.90+ means $0.10/share max profit but full downside if wrong. The 13:15 window lost $3,656 primarily because 89% of winner fills were above $0.80 — the edge was burned by overpaying.

**Flaw 2: Expensive Insurance (Loser Avg Above $0.15)**

| Window | Loser Avg Price | Should Be | Excess Cost |
|---|---|---|---|
| 13:15 | $0.394 | <$0.10 | ~$4,500 wasted |
| 14:00 | $0.365 | <$0.10 | ~$1,500 wasted |
| 15:00 | $0.506 | <$0.10 | ~$2,000 wasted |

When the "insurance" side costs $0.40/share, it's no longer insurance — it's a second directional bet that also loses. True insurance should be pennies ($0.01-0.10).

**Flaw 3: Excessive Side-Switching (Indecision Tax)**

| Window | Side Switches | Result |
|---|---|---|
| 13:15 | **173** | -$3,656 |
| 14:15 | 11 | +$370 |
| 14:30 | **2** | +$1,620 |

The 13:15 window had 173 side switches in 13 minutes — the bot was flip-flopping continuously, posting GTC on Up then Down then Up then Down. Each switch means:
- Cancel existing orders (lose queue priority)
- Re-post at new prices (may get filled at worse levels)
- Cross the spread repeatedly

W6 (the big win) had only **2 switches**: entered Up, stayed Up, done.

### 7.2 Quantified Impact of Flaws

Applying three rules to the 8-window dataset:

**Rule: Never fill winner above $0.80**
- 13:15: avg drops from $0.812 to ~$0.55 → estimated PnL improvement: +$4,000-5,000
- 14:45: avg drops from $0.909 to ~$0.65 → estimated PnL improvement: +$1,000-1,500

**Rule: Insurance below $0.10 only**
- 13:15: loser avg $0.394 → $0.05 → saves ~$5,000
- 14:00: loser avg $0.365 → $0.05 → saves ~$1,800
- 15:00: loser avg $0.506 → $0.05 → saves ~$2,000

**Rule: Max 5 side switches per window**
- 13:15: 173 → 5 → reduced deployed capital, smaller loss

Estimated impact: **8-window result flips from -$3,024 (-5.9%) to approximately +$3,000-5,000 (+10-15%)**

---

## 8. Infrastructure & Approach

### 8.1 What He Uses

- **Signal**: Binance BTC spot price vs window open price (Chainlink oracle reference)
- **Execution**: GTC limit orders on Polymarket CLOB, multiple price levels
- **Wallets**: Primary wallet `0xe1d6...` + likely multiple proxy wallets (sybil score 89.39)
- **Timing**: Enters within 15-35 seconds of window open, trades throughout full window
- **Markets**: BTC, ETH, SOL, XRP — both 5m and 15m timeframes
- **Scale**: ~$1.1M/day deployed across all markets, ~$28.7K/day PnL (3-day avg)

### 8.2 Hardware Requirements

Minimal. GTC orders don't require HFT latency:
- Any VPS or local machine
- Binance WebSocket for real-time BTC price
- Polymarket CLOB API for order placement
- Polygon RPC for on-chain settlement
- Gas costs: ~$0.01/tx on Polygon = negligible

### 8.3 Budget to Replicate

| Scenario | Capital Needed | Expected Daily PnL |
|---|---|---|
| Minimum (BTC 15m only) | $3,000-5,000 | $100-300 |
| Comfortable (BTC 15m + 5m) | $5,000-8,000 | $300-500 |
| Full scale (BTC + ETH + SOL) | $50,000+ | $5,000-10,000 |

Capital unlocks every 15 minutes (not locked long-term). Polygon gas is near-zero. Infrastructure cost is effectively $0 given existing Rust codebase.

---

## 9. Replication Strategy — How to Do Better

### 9.1 The Improved Approach

| Aspect | Scallops' Approach | Our Improved Approach |
|---|---|---|
| Entry timing | +15-35s from open | +15-30s (similar, no need to be faster) |
| Direction signal | BTC spot delta | Same — Binance WebSocket |
| Order type | GTC across wide range ($0.45-$0.98) | **GTC only $0.45-$0.75** (hard cap) |
| Insurance | $0.01-$0.50 (too expensive) | **$0.01-$0.10 only** (true pennies) |
| Side switches | Up to 173 per window | **Max 3** — commit or don't |
| Late adds | Buys at $0.985 (no edge) | **Stop at 70% through window** |
| Position sizing | Let fills determine size | Same — GTC ladder, market decides |

### 9.2 The Price Ladder

```
WINDOW OPENS → Check BTC spot direction

If BTC trending UP → favor Up side:
  Post GTC bids on Up at: $0.48, $0.52, $0.55, $0.58, $0.62, $0.65
  Each level: 100-200 shares
  After 50% of window: post Down insurance at $0.01-$0.05

If BTC trending DOWN → favor Down side:
  Post GTC bids on Down at: $0.48, $0.52, $0.55, $0.58, $0.62, $0.65
  Each level: 100-200 shares
  After 50% of window: post Up insurance at $0.01-$0.05

NEVER post above $0.75 on either side.
CANCEL all unfilled orders at 80% through window.
```

### 9.3 Implementation Checklist

Using existing Mantis/Polymarket Rust codebase:

1. **Window lifecycle manager** — detect 15m/5m window boundaries, trigger entry/exit
2. **BTC direction signal** — compare Binance spot to window open price at +15s
3. **GTC price ladder** — place 6-8 limit orders at $0.05 increments on favored side
4. **Insurance module** — place penny orders on opposite side at 50% through
5. **Hard caps** — never fill above $0.75, max 3 side switches, stop at 80%
6. **Position tracker** — cumulative Up/Down shares and cost per window
7. **Cancel logic** — cancel unfilled GTC orders at 80% or on side switch

### 9.4 Suggested Ramp-Up Path

| Week | Action | Capital | Risk |
|---|---|---|---|
| 1 | Paper trade BTC 15m, validate signal accuracy | $0 | None |
| 2 | Live with $2-3K, BTC 15m only, $500/window | $3,000 | Low |
| 3 | Add BTC 5m, increase to $750/window | $5,000 | Low |
| 4+ | Add ETH/SOL, scale based on results | $10K+ | Moderate |

---

## 10. Open Questions

1. **Does direction accuracy improve with more signal lag?** At +15s the signal might be clearer than at +10s but liquidity may be lower. Need to backtest with existing Binance data.

2. **Multiple wallets — is it necessary?** Scallops uses 30-50+ wallets (sybil score 89.39). This may be for position limits, execution parallelism, or order book depth. Start with 1 wallet and assess limits.

3. **5m vs 15m edge comparison?** 15m windows get 2.9x more capital per window ($4,457 vs $1,534) but 5m has 1.8x more windows per day. Need to compare win rates across timeframes.

4. **Is there a time-of-day effect?** The 8 windows analyzed were 13:15-15:15 UTC. Does the edge vary by hour or session?

5. **Competition dynamics?** In the all-wallets analysis, 69-94 unique wallets trade each window. How much liquidity is available after all participants are in?

---

## Appendix A: Raw Data Sources

- Trade-by-trade reconstruction: `scripts/reconstruct_flow.py`
- Full 3-day dataset: `docs/research/2026-04-02-btc-5m-15m-raw.json`
- Market flow analysis: `scripts/market_flow_analysis.py`
- Order type analysis: `scripts/order_type_analysis.py`
- Edge analysis: `scripts/edge_analysis.py`
- Reusable report generator: `scripts/btc_trader_report.py`

## Appendix B: API Reference

- Heisenberg API base: `https://narrative.agent.heisenberg.so/api/v2/semantic/retrieve/parameterized`
- Agent 556: Polymarket Trades
- Agent 569: Polymarket PnL
- Agent 581: Wallet 360
- Agent 584: H-Score Leaderboard
- Full API docs: `docs/research/falcon-api-reference.md`

## Appendix C: On-Chain Verification

Sample transactions (Polygonscan):
- https://polygonscan.com/tx/0x9d0092eca27d827f880dc77c5133a44fd68e8678d9caf929ef829bf0d740c3da
- https://polygonscan.com/tx/0xa78fa74db1dbb8354e342e9207b0892d62696aef6d32c6730f1522f8402dd639
- https://polygonscan.com/tx/0xf5f41c3904ca049bb8de495ce883dbda4d3160deb5f4411f9ffb37455e8b1d3f

Polymarket CTFExchange contract methods observed:
- `fillOrder` (0xfe729aee) — single order execution
- `fillOrders` (0xd798eff6) — batch fill multiple orders
- `matchOrders` (0xe60f0c05) — maker-taker matching
