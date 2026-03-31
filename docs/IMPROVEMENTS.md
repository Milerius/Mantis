# Future Improvements — Based on Target Trader Analysis

**Target**: 0xe1D6b51521Bd4365769199f392F9818661BD907
**Their stats**: $93,671 PnL, $3,122/day, 0.68% edge per dollar traded
**Analysis date**: 2026-03-29

---

## Speed Requirements

The target trader is NOT a sub-millisecond HFT bot. Their entry timing:

| Entry Price | Timing | Our Speed Needed | Count |
|---|---|---|---|
| $0.43-0.50 | First 30-90s of window | 10-30 seconds | 23% of trades |
| $0.50-0.65 | First 1-3 minutes | 1-3 minutes | 43% of trades |
| $0.65-0.85 | Minute 3-7+ | Not speed-dependent | 33% of trades |

**Conclusion**: Seconds-level latency is sufficient. Our Rust bot's tick-to-order path
of ~5-11ms is 100x faster than needed. The edge is in **signal quality**, not speed.

A $20/month Hetzner VPS in US-East is more than adequate.

---

## Strategy Improvements (Priority Order)

### P0: Add 5m Window Support

The target trader makes 14 of 30 trades on 5m windows (47%!).
- 5m windows resolve 3x faster than 15m = 3x more capital recycling
- They achieve 100% win rate on 5m with avg $0.565 entry
- 5m windows are MORE profitable per-trade ($275 avg) than 15m ($330 avg)
  at smaller position size ($612 vs $1,372)

**Action**: Download 5m PBT data, backtest, add to paper trading config.

### P1: Raise Momentum Entry Price Threshold

Current MomentumConfirmation caps at $0.72. Target trader enters at $0.767-0.848.
With 80%+ win rate, entries up to $0.80 are still profitable.

**Action**: Raise max_entry_price to $0.80 for momentum strategy.

### P2: Multi-Asset (SOL, XRP)

Target trader trades BTC + ETH + SOL + XRP simultaneously.
BTC: $22K deployed, ETH: $6.8K, SOL: $1.5K, XRP: $0.5K.
Position sizes scale with asset's volume/liquidity.

**Action**: Enable SOL in config (already implemented, just disabled).
Test XRP with small positions.

### P3: Multi-Timeframe Stacking (4h + Daily)

Target trader stacks same thesis across 5m/15m/4h/daily.
When BTC trends down for an hour, all timeframes align.
4h/daily positions are larger ($4,694 daily) but lower frequency.

**Action**: Add 4h and daily market scanning. Requires different Gamma API
slug patterns for these timeframes.

### P4: Direction Switching Logic

Target trader switches Up/Down EVERY FEW WINDOWS:
- 16:00 Down, 16:05 Up, 16:10 Down, 16:15 Up, 16:20 Down, 16:25 Up...

This confirms: they follow spot price movement, not predict ahead.
Each 5m window is an independent decision based on current spot direction.

**Action**: Current EarlyDirectional already does this (follows spot_direction).
No code change needed — just confirmation our approach is correct.

### P5: Volatility Regime Filter

From our data analysis (146 markets):
- High vol (>0.1% in 1min): direction predicts outcome 81.8%
- Low vol (<=0.1% in 1min): direction predicts only 57.3%

**Action**: Add volatility gate: only trade when first-minute magnitude > 0.1%.
Expected impact: fewer trades but higher win rate (81% → 85%+).

### P6: Anti-Reversal Magnitude Filter

24.7% of windows reverse after minute 3. Reversals correlate with LOW magnitude.
Small spot moves (< 0.1%) are more likely to reverse.

**Action**: Require higher magnitude threshold before entering.
Or: reduce position size when magnitude is borderline.

### P7: Scale Position Sizes

Target trader: $612-4,694 per position. Our cap: $25 (micro-testing).
Once paper trading validates, scale to:
- Phase 1: $100-500 per trade
- Phase 2: $500-2,000 per trade
- Phase 3: $2,000-5,000 per trade (matching target trader)

**Action**: Config change only. No code changes needed.

### P8: Orderbook Imbalance Signal

Use full orderbook depth (we have it from PBT) to detect buy/sell imbalance.
When bid depth >> ask depth, price likely to rise.

**Action**: Analyze PBT orderbook depth data. Compute imbalance signal.
Add as a feature to strategy evaluation.

### P9: Cross-Window Momentum

Track if consecutive windows trend in the same direction.
If window N resolved Up, does window N+1 also go Up?
Serial correlation could boost entry confidence.

**Action**: Analyze PBT data for consecutive window correlation.

---

## Infrastructure Notes

- **Server**: Hetzner CPX31, US-East (Ashburn), $17/month
- **Latency**: <5ms to Polymarket/Binance = sufficient
- **Bare metal NOT needed**: target trader operates at seconds, not milliseconds
- **24/7 uptime**: systemd service on Linux
- **Monitoring**: OpenClaw agent (Phase 4) for Telegram alerts
