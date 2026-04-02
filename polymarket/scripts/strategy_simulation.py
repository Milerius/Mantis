#!/usr/bin/env python3
"""
Quantitative Strategy Simulation — BTC 15m Up/Down

Validates the proposed strategy parameters using:
1. Replay of real Scallops windows (28 windows from our analysis)
2. Monte Carlo simulation across different signal accuracies
3. Sensitivity analysis on key parameters (max price, hedge ratio, insurance)

The goal: prove the math works BEFORE building the bot.
"""

import random
import math
from dataclasses import dataclass, field
from typing import List, Tuple

random.seed(42)

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 1: THE MATH — How binary option P&L works
# ═══════════════════════════════════════════════════════════════════════════════

print("=" * 100)
print("SECTION 1: THE FUNDAMENTAL MATH")
print("=" * 100)
print("""
Binary option: pays $1 if correct, $0 if wrong.

If you buy W shares of WINNER at avg price p_w, and L shares of LOSER at avg price p_l:

  PnL = W × (1 - p_w) - L × p_l
        ──────────────   ────────
        winner profit    loser cost
        (always > 0)     (always < 0)

  Total deployed = W × p_w + L × p_l
  ROI = PnL / Total deployed

KEY INSIGHT: Your edge per winning share = (1 - p_w)
  At p_w = 0.50 → edge = $0.50/share (100% return if correct)
  At p_w = 0.70 → edge = $0.30/share (43% return if correct)
  At p_w = 0.90 → edge = $0.10/share (11% return if correct)
  At p_w = 0.98 → edge = $0.02/share (2% return if correct)

The LOSER side always costs you L × p_l with zero return.
So total PnL = winner_edge × winner_shares - loser_total_cost.
""")


# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 2: SINGLE WINDOW MATH — Prove the formulas
# ═══════════════════════════════════════════════════════════════════════════════

print("=" * 100)
print("SECTION 2: SINGLE WINDOW P&L EXAMPLES")
print("=" * 100)


@dataclass
class Position:
    """Tracks a position in a single window."""
    up_shares: float = 0.0
    up_cost: float = 0.0
    down_shares: float = 0.0
    down_cost: float = 0.0

    @property
    def total_deployed(self):
        return self.up_cost + self.down_cost

    def buy(self, side: str, shares: float, price: float):
        cost = shares * price
        if side == "Up":
            self.up_shares += shares
            self.up_cost += cost
        else:
            self.down_shares += shares
            self.down_cost += cost

    def pnl_if(self, winner: str) -> float:
        if winner == "Up":
            return self.up_shares - self.up_cost - self.down_cost
        else:
            return self.down_shares - self.down_cost - self.up_cost

    def recovery_if(self, winner: str) -> float:
        """How much capital comes back."""
        if winner == "Up":
            return self.up_shares
        else:
            return self.down_shares

    def summary(self, winner: str) -> str:
        pnl = self.pnl_if(winner)
        recovery = self.recovery_if(winner)
        deployed = self.total_deployed
        roi = pnl / deployed * 100 if deployed > 0 else 0
        rec_pct = recovery / deployed * 100 if deployed > 0 else 0
        up_avg = self.up_cost / self.up_shares if self.up_shares > 0 else 0
        dn_avg = self.down_cost / self.down_shares if self.down_shares > 0 else 0
        return (
            f"  Up:  {self.up_shares:>6.0f} shares @ avg ${up_avg:.3f} = ${self.up_cost:>7,.0f}\n"
            f"  Dn:  {self.down_shares:>6.0f} shares @ avg ${dn_avg:.3f} = ${self.down_cost:>7,.0f}\n"
            f"  Deployed: ${deployed:>7,.0f} | Winner: {winner}\n"
            f"  PnL: ${pnl:>+7,.0f} ({roi:>+.1f}%) | Recovery: {rec_pct:.0f}%"
        )


# Example 1: Our proposed approach (correct direction)
print("\n--- Example 1: We bet UP, UP wins (correct) ---")
pos1 = Position()
# Favored ladder: Up at $0.45-0.70
for price in [0.45, 0.50, 0.55, 0.60, 0.65, 0.70]:
    pos1.buy("Up", 100, price)
# Hedge: Down at $0.45-0.50
for price in [0.45, 0.50]:
    pos1.buy("Down", 40, price)
# Insurance: Down at pennies
for price in [0.02, 0.05, 0.08]:
    pos1.buy("Down", 50, price)

print(pos1.summary("Up"))

# Example 2: Same position, but we're WRONG (Down wins)
print("\n--- Example 2: We bet UP, DOWN wins (wrong) ---")
print(pos1.summary("Down"))

# Example 3: Scallops-style (chasing to $0.95)
print("\n--- Example 3: Scallops chasing approach — bet UP at $0.50-0.95, UP wins ---")
pos3 = Position()
for price in [0.50, 0.55, 0.60, 0.65, 0.70, 0.75, 0.80, 0.85, 0.90, 0.95]:
    pos3.buy("Up", 100, price)
for price in [0.45, 0.50]:
    pos3.buy("Down", 40, price)
for price in [0.02, 0.05, 0.08]:
    pos3.buy("Down", 50, price)
print(pos3.summary("Up"))

print("\n--- Example 4: Scallops chasing — bet UP at $0.50-0.95, DOWN wins (wrong) ---")
print(pos3.summary("Down"))

# Compare
print("\n--- COMPARISON ---")
our_right = pos1.pnl_if("Up")
our_wrong = pos1.pnl_if("Down")
sc_right = pos3.pnl_if("Up")
sc_wrong = pos3.pnl_if("Down")
print(f"  {'':>20} | {'Correct':>10} | {'Wrong':>10} | {'Ratio':>8}")
print(f"  {'Our approach':>20} | ${our_right:>+8,.0f} | ${our_wrong:>+8,.0f} | {abs(our_right/our_wrong) if our_wrong != 0 else 'inf':>7.2f}x")
print(f"  {'Scallops (chase)':>20} | ${sc_right:>+8,.0f} | ${sc_wrong:>+8,.0f} | {abs(sc_right/sc_wrong) if sc_wrong != 0 else 'inf':>7.2f}x")
print(f"\n  At 55% accuracy, expected value per window:")
print(f"    Ours:     0.55 × ${our_right:+,.0f} + 0.45 × ${our_wrong:+,.0f} = ${0.55*our_right + 0.45*our_wrong:+,.0f}")
print(f"    Scallops: 0.55 × ${sc_right:+,.0f} + 0.45 × ${sc_wrong:+,.0f} = ${0.55*sc_right + 0.45*sc_wrong:+,.0f}")


# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 3: MONTE CARLO — How does accuracy affect profitability?
# ═══════════════════════════════════════════════════════════════════════════════

print("\n\n" + "=" * 100)
print("SECTION 3: MONTE CARLO SIMULATION — Signal Accuracy vs Profitability")
print("=" * 100)


@dataclass
class StrategyConfig:
    """Configuration for a trading strategy."""
    name: str
    favored_prices: List[float]
    favored_shares: float
    hedge_prices: List[float]
    hedge_shares: float
    insurance_prices: List[float]
    insurance_shares: float

    def build_position(self) -> Position:
        pos = Position()
        for p in self.favored_prices:
            pos.buy("Up", self.favored_shares, p)  # "Up" = favored side
        for p in self.hedge_prices:
            pos.buy("Down", self.hedge_shares, p)
        for p in self.insurance_prices:
            pos.buy("Down", self.insurance_shares, p)
        return pos


# Define strategies to compare
strategies = {
    "ours_conservative": StrategyConfig(
        name="Ours (max $0.70)",
        favored_prices=[0.45, 0.50, 0.55, 0.60, 0.65, 0.70],
        favored_shares=100,
        hedge_prices=[0.45, 0.50],
        hedge_shares=40,
        insurance_prices=[0.02, 0.05, 0.08],
        insurance_shares=50,
    ),
    "ours_moderate": StrategyConfig(
        name="Ours (max $0.75)",
        favored_prices=[0.45, 0.50, 0.55, 0.60, 0.65, 0.70, 0.75],
        favored_shares=100,
        hedge_prices=[0.45, 0.50],
        hedge_shares=40,
        insurance_prices=[0.02, 0.05, 0.08],
        insurance_shares=50,
    ),
    "scallops_chase": StrategyConfig(
        name="Scallops (chase to $0.95)",
        favored_prices=[0.50, 0.55, 0.60, 0.65, 0.70, 0.75, 0.80, 0.85, 0.90, 0.95],
        favored_shares=100,
        hedge_prices=[0.45, 0.50],
        hedge_shares=40,
        insurance_prices=[0.02, 0.05, 0.08],
        insurance_shares=50,
    ),
    "no_hedge": StrategyConfig(
        name="No hedge (favored only)",
        favored_prices=[0.45, 0.50, 0.55, 0.60, 0.65, 0.70],
        favored_shares=100,
        hedge_prices=[],
        hedge_shares=0,
        insurance_prices=[],
        insurance_shares=0,
    ),
    "heavy_hedge": StrategyConfig(
        name="Heavy hedge (50/50)",
        favored_prices=[0.45, 0.50, 0.55, 0.60, 0.65, 0.70],
        favored_shares=100,
        hedge_prices=[0.45, 0.50, 0.55, 0.60],
        hedge_shares=80,
        insurance_prices=[0.02, 0.05, 0.08],
        insurance_shares=50,
    ),
    "penny_only_insurance": StrategyConfig(
        name="Ours + penny insurance only ($0.02)",
        favored_prices=[0.45, 0.50, 0.55, 0.60, 0.65, 0.70],
        favored_shares=100,
        hedge_prices=[],
        hedge_shares=0,
        insurance_prices=[0.01, 0.02, 0.03, 0.05],
        insurance_shares=80,
    ),
}


def simulate_strategy(config: StrategyConfig, accuracy: float, n_windows: int = 10000) -> dict:
    """Simulate n_windows with given signal accuracy."""
    pos_template = config.build_position()

    # Pre-compute PnL for correct and wrong
    pnl_correct = pos_template.pnl_if("Up")  # favored = Up = correct
    pnl_wrong = pos_template.pnl_if("Down")  # favored = Up but Down wins = wrong
    deployed = pos_template.total_deployed

    total_pnl = 0
    wins = 0
    losses = 0
    max_drawdown = 0
    running_pnl = 0
    peak = 0
    pnls = []

    for _ in range(n_windows):
        if random.random() < accuracy:
            pnl = pnl_correct
            wins += 1
        else:
            pnl = pnl_wrong
            losses += 1

        total_pnl += pnl
        running_pnl += pnl
        peak = max(peak, running_pnl)
        drawdown = peak - running_pnl
        max_drawdown = max(max_drawdown, drawdown)
        pnls.append(pnl)

    avg_pnl = total_pnl / n_windows
    total_deployed = deployed * n_windows
    roi = total_pnl / total_deployed * 100

    # Sharpe-like ratio (per window)
    mean = sum(pnls) / len(pnls)
    variance = sum((p - mean) ** 2 for p in pnls) / len(pnls)
    std = math.sqrt(variance) if variance > 0 else 0.001
    sharpe = mean / std

    return {
        "accuracy": accuracy,
        "wins": wins,
        "losses": losses,
        "total_pnl": total_pnl,
        "avg_pnl": avg_pnl,
        "deployed_per_window": deployed,
        "roi_per_window": avg_pnl / deployed * 100 if deployed > 0 else 0,
        "max_drawdown": max_drawdown,
        "sharpe": sharpe,
        "pnl_correct": pnl_correct,
        "pnl_wrong": pnl_wrong,
        "breakeven_accuracy": abs(pnl_wrong) / (abs(pnl_correct) + abs(pnl_wrong)),
    }


# Run simulations across accuracy levels
print(f"\n  Simulating 10,000 windows per accuracy level per strategy...\n")

accuracy_levels = [0.40, 0.45, 0.50, 0.52, 0.55, 0.58, 0.60, 0.65, 0.70]

# Header
header = f"  {'Strategy':>30} | {'Acc':>5} | {'Win$':>8} | {'Loss$':>8} | {'BE Acc':>6} | {'Avg PnL':>8} | {'ROI/win':>7} | {'MaxDD':>8} | {'Sharpe':>6}"
print(header)
print("  " + "-" * (len(header) - 2))

for strat_key in ["ours_conservative", "ours_moderate", "scallops_chase", "no_hedge", "heavy_hedge", "penny_only_insurance"]:
    config = strategies[strat_key]
    for acc in accuracy_levels:
        result = simulate_strategy(config, acc)
        if acc in [0.40, 0.50, 0.55, 0.60, 0.70]:
            pnl_marker = "+" if result["avg_pnl"] > 0 else " "
            print(f"  {config.name:>30} | {acc:>4.0%} | ${result['pnl_correct']:>+6,.0f} | "
                  f"${result['pnl_wrong']:>+6,.0f} | {result['breakeven_accuracy']:>5.1%} | "
                  f"${result['avg_pnl']:>+7,.1f} | {result['roi_per_window']:>+5.1f}% | "
                  f"${result['max_drawdown']:>7,.0f} | {result['sharpe']:>5.2f}")
    print()


# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 4: BREAKEVEN ANALYSIS
# ═══════════════════════════════════════════════════════════════════════════════

print("\n" + "=" * 100)
print("SECTION 4: BREAKEVEN ACCURACY — What win rate do we need?")
print("=" * 100)

print(f"\n  {'Strategy':>30} | {'Win PnL':>8} | {'Loss PnL':>9} | {'Breakeven':>9} | {'Note':>30}")
print("  " + "-" * 100)

for strat_key, config in strategies.items():
    pos = config.build_position()
    pnl_w = pos.pnl_if("Up")
    pnl_l = pos.pnl_if("Down")
    be = abs(pnl_l) / (abs(pnl_w) + abs(pnl_l))
    deployed = pos.total_deployed

    note = ""
    if be < 0.45:
        note = "EXCELLENT — profitable even at 45%"
    elif be < 0.50:
        note = "GOOD — profitable below coin flip"
    elif be < 0.55:
        note = "OK — needs slight edge"
    else:
        note = "RISKY — needs strong signal"

    print(f"  {config.name:>30} | ${pnl_w:>+6,.0f} | ${pnl_l:>+7,.0f} | {be:>8.1%} | {note}")


# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 5: SENSITIVITY ANALYSIS — Which parameters matter most?
# ═══════════════════════════════════════════════════════════════════════════════

print("\n\n" + "=" * 100)
print("SECTION 5: SENSITIVITY ANALYSIS")
print("=" * 100)

# 5a: Max favored price sensitivity
print("\n--- 5a: Max favored price (holding everything else constant) ---")
print(f"  {'Max Price':>10} | {'Win PnL':>8} | {'Loss PnL':>9} | {'Breakeven':>9} | {'Deployed':>9} | {'Win ROI':>8}")
print("  " + "-" * 70)

base_prices = [0.45, 0.50, 0.55, 0.60]
for max_p_cents in range(60, 100, 5):
    max_p = max_p_cents / 100
    prices = base_prices + [p / 100 for p in range(65, max_p_cents + 1, 5)]
    pos = Position()
    for p in prices:
        pos.buy("Up", 100, p)
    for p in [0.45, 0.50]:
        pos.buy("Down", 40, p)
    for p in [0.02, 0.05, 0.08]:
        pos.buy("Down", 50, p)

    pnl_w = pos.pnl_if("Up")
    pnl_l = pos.pnl_if("Down")
    be = abs(pnl_l) / (abs(pnl_w) + abs(pnl_l)) if (abs(pnl_w) + abs(pnl_l)) > 0 else 1
    win_roi = pnl_w / pos.total_deployed * 100 if pos.total_deployed > 0 else 0

    marker = " <<<" if max_p <= 0.75 else ""
    print(f"  ${max_p:>9.2f} | ${pnl_w:>+6,.0f} | ${pnl_l:>+7,.0f} | {be:>8.1%} | ${pos.total_deployed:>8,.0f} | {win_roi:>+6.1f}%{marker}")


# 5b: Hedge ratio sensitivity
print("\n--- 5b: Hedge ratio (hedge shares as % of favored) ---")
print(f"  {'Hedge %':>10} | {'Win PnL':>8} | {'Loss PnL':>9} | {'Breakeven':>9} | {'Deployed':>9} | {'Loss %':>8}")
print("  " + "-" * 70)

favored_shares = 100
for hedge_pct in [0, 10, 20, 30, 40, 50, 60, 80, 100]:
    hedge_sh = favored_shares * hedge_pct / 100
    pos = Position()
    for p in [0.45, 0.50, 0.55, 0.60, 0.65, 0.70]:
        pos.buy("Up", favored_shares, p)
    for p in [0.45, 0.50]:
        pos.buy("Down", hedge_sh, p)
    for p in [0.02, 0.05, 0.08]:
        pos.buy("Down", 50, p)

    pnl_w = pos.pnl_if("Up")
    pnl_l = pos.pnl_if("Down")
    be = abs(pnl_l) / (abs(pnl_w) + abs(pnl_l)) if (abs(pnl_w) + abs(pnl_l)) > 0 else 1
    loss_pct = pnl_l / pos.total_deployed * 100 if pos.total_deployed > 0 else 0

    print(f"  {hedge_pct:>9}% | ${pnl_w:>+6,.0f} | ${pnl_l:>+7,.0f} | {be:>8.1%} | ${pos.total_deployed:>8,.0f} | {loss_pct:>+6.1f}%")


# 5c: Insurance size sensitivity
print("\n--- 5c: Insurance shares per level (at $0.02/$0.05/$0.08) ---")
print(f"  {'Ins Shares':>10} | {'Win PnL':>8} | {'Loss PnL':>9} | {'Breakeven':>9} | {'Ins Cost':>9} | {'Note':>20}")
print("  " + "-" * 85)

for ins_shares in [0, 20, 50, 80, 100, 150, 200, 500]:
    pos = Position()
    for p in [0.45, 0.50, 0.55, 0.60, 0.65, 0.70]:
        pos.buy("Up", 100, p)
    for p in [0.45, 0.50]:
        pos.buy("Down", 40, p)
    ins_cost = 0
    for p in [0.02, 0.05, 0.08]:
        pos.buy("Down", ins_shares, p)
        ins_cost += ins_shares * p

    pnl_w = pos.pnl_if("Up")
    pnl_l = pos.pnl_if("Down")
    be = abs(pnl_l) / (abs(pnl_w) + abs(pnl_l)) if (abs(pnl_w) + abs(pnl_l)) > 0 else 1

    # If wrong, insurance pays: ins_shares * 3 levels of Down shares redeem at $1
    ins_payoff_if_wrong = ins_shares * 3  # 3 price levels, ins_shares each

    note = f"ins saves ${ins_payoff_if_wrong - ins_cost:.0f} if wrong"
    print(f"  {ins_shares:>9} | ${pnl_w:>+6,.0f} | ${pnl_l:>+7,.0f} | {be:>8.1%} | ${ins_cost:>8,.0f} | {note}")


# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 6: BANKROLL SIMULATION — How much capital do we need?
# ═══════════════════════════════════════════════════════════════════════════════

print("\n\n" + "=" * 100)
print("SECTION 6: BANKROLL SIMULATION — Capital Requirements")
print("=" * 100)

config = strategies["ours_conservative"]
pos = config.build_position()
pnl_correct = pos.pnl_if("Up")
pnl_wrong = pos.pnl_if("Down")
deployed_per = pos.total_deployed

print(f"\n  Strategy: {config.name}")
print(f"  Deployed per window: ${deployed_per:,.0f}")
print(f"  PnL if correct: ${pnl_correct:+,.0f}")
print(f"  PnL if wrong: ${pnl_wrong:+,.0f}")

for accuracy in [0.52, 0.55, 0.58, 0.60]:
    print(f"\n  --- At {accuracy:.0%} accuracy ---")

    n_sims = 5000
    n_windows = 500  # ~5 days of trading (96 windows/day)

    final_bankrolls = []
    min_bankrolls = []
    ruin_count = 0

    for _ in range(n_sims):
        bankroll = 5000  # starting capital
        min_br = bankroll
        peak_br = bankroll

        for w in range(n_windows):
            if bankroll < deployed_per:
                ruin_count += 1
                break

            if random.random() < accuracy:
                bankroll += pnl_correct
            else:
                bankroll += pnl_wrong

            min_br = min(min_br, bankroll)
            peak_br = max(peak_br, bankroll)

        final_bankrolls.append(bankroll)
        min_bankrolls.append(min_br)

    final_bankrolls.sort()
    min_bankrolls.sort()

    print(f"    Starting: $5,000 | Windows: {n_windows} (~5 days)")
    print(f"    Final bankroll:")
    print(f"      Worst 5%:  ${final_bankrolls[int(n_sims * 0.05)]:>10,.0f}")
    print(f"      25th pct:  ${final_bankrolls[int(n_sims * 0.25)]:>10,.0f}")
    print(f"      Median:    ${final_bankrolls[int(n_sims * 0.50)]:>10,.0f}")
    print(f"      75th pct:  ${final_bankrolls[int(n_sims * 0.75)]:>10,.0f}")
    print(f"      Best 5%:   ${final_bankrolls[int(n_sims * 0.95)]:>10,.0f}")
    print(f"    Max drawdown (worst point):")
    print(f"      Worst 5%:  ${min_bankrolls[int(n_sims * 0.05)]:>10,.0f} (lost ${5000 - min_bankrolls[int(n_sims * 0.05)]:,.0f})")
    print(f"      Median:    ${min_bankrolls[int(n_sims * 0.50)]:>10,.0f} (lost ${5000 - min_bankrolls[int(n_sims * 0.50)]:,.0f})")
    print(f"    Ruin probability: {ruin_count / n_sims * 100:.2f}%")


# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 7: REPLAY ON REAL DATA — Apply our rules to the 20 measured windows
# ═══════════════════════════════════════════════════════════════════════════════

print("\n\n" + "=" * 100)
print("SECTION 7: REPLAY ON REAL SCALLOPS DATA")
print("=" * 100)

# Real data from our 20 measurable windows
# (w_avg, l_avg, w_cost, l_cost, w_shares, winner, scallops_pnl)
real_windows = [
    # 10 new windows (08:45-11:00)
    (0.678, 0.348, 3463, 1511, 5107, "Down", 455),     # 08:45 WIN
    (0.569, 0.196, 3060, 1189, 5378, "Down", 1164),    # 09:00 WIN
    (0.945, 0.176, 10647, 779, 11267, "Up", -1451),     # 09:15 LOSS
    (0.619, 0.360, 4382, 2359, 7080, "Down", 346),      # 09:30 WIN
    (0.555, 0.461, 2235, 2235, 4026, "Up", -479),       # 09:45 LOSS
    (0.879, 0.248, 2586, 1004, 2942, "Up", -661),       # 10:30 LOSS
    (0.652, 0.305, 1934, 829, 2967, "Down", 218),       # 10:45 WIN
    # 8 previous windows (11:30-15:15)
    (0.630, 0.041, 1550, 82, 2460, "Down", 821),        # 11:30 WIN
    (0.301, 0.642, 831, 1930, 2761, "Up", -239),        # 11:45 LOSS
    (0.567, 0.331, 4056, 2452, 7150, "Down", 648),      # 12:00 WIN
    (0.533, 0.186, 2464, 733, 4624, "Down", 1427),      # 12:15 WIN
    (0.963, 0.204, 6540, 1605, 6789, "Up", -1356),      # 12:30 LOSS
    (0.142, 0.547, 341, 841, 2403, "Up", 1220),         # 12:45 WIN
    (0.538, 0.429, 3123, 2179, 5800, "Down", 498),      # 13:00 WIN
    (0.812, 0.394, 11052, 6216, 13611, "Up", -3656),    # 13:15 LOSS
    (0.633, 0.365, 3541, 2468, 5593, "Up", -416),       # 14:00 LOSS
    (0.661, 0.297, 2918, 1124, 4412, "Down", 370),      # 14:15 WIN
    (0.909, 0.224, 4472, 1363, 4917, "Down", -917),     # 14:45 LOSS
    (0.568, 0.506, 3086, 2343, 5429, "Up", 655),        # 15:00 WIN
    (0.717, 0.099, 2703, 269, 3771, "Down", 783),       # 15:15 WIN
]

print(f"\n  Replaying {len(real_windows)} real windows...")
print(f"\n  Question: If we applied the '$0.75 max price' rule to Scallops' actual positions,")
print(f"  what would his P&L have been?\n")

scallops_total_pnl = sum(w[6] for w in real_windows)
filtered_total_pnl = 0
filtered_count = 0
skipped_count = 0

print(f"  {'#':>3} | {'W.Avg':>6} | {'Scallops PnL':>12} | {'Rule':>10} | {'Note'}")
print("  " + "-" * 70)

for i, (w_avg, l_avg, w_cost, l_cost, w_shares, winner, sc_pnl) in enumerate(real_windows, 1):
    if w_avg <= 0.75:
        filtered_total_pnl += sc_pnl
        filtered_count += 1
        rule = "KEEP"
        note = ""
    else:
        skipped_count += 1
        rule = "SKIP"
        note = f"(would have avoided ${-sc_pnl:+,.0f} loss)" if sc_pnl < 0 else f"(missed ${sc_pnl:+,.0f} win)"
        # If we skip this window entirely, PnL = 0

    print(f"  {i:>3} | ${w_avg:.3f} | ${sc_pnl:>+10,.0f} | {rule:>10} | {note}")

print(f"\n  RESULT:")
print(f"    Scallops actual PnL:        ${scallops_total_pnl:>+10,.0f} ({len(real_windows)} windows)")
print(f"    With $0.75 rule:            ${filtered_total_pnl:>+10,.0f} ({filtered_count} windows traded, {skipped_count} skipped)")
print(f"    Improvement:                ${filtered_total_pnl - scallops_total_pnl:>+10,.0f}")
print(f"    Avoided losses:             ${sum(-w[6] for w in real_windows if w[0] > 0.75 and w[6] < 0):>+10,.0f}")


# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 8: RECOMMENDED PARAMETERS
# ═══════════════════════════════════════════════════════════════════════════════

print("\n\n" + "=" * 100)
print("SECTION 8: RECOMMENDED STRATEGY PARAMETERS")
print("=" * 100)

# Final recommended config
rec = strategies["ours_conservative"]
rec_pos = rec.build_position()
rec_w = rec_pos.pnl_if("Up")
rec_l = rec_pos.pnl_if("Down")
rec_be = abs(rec_l) / (abs(rec_w) + abs(rec_l))

print(f"""
  RECOMMENDED CONFIGURATION:

  Favored Side (directional bet):
    Prices:     $0.45, $0.50, $0.55, $0.60, $0.65, $0.70
    Shares:     100 per level
    Max price:  $0.75 HARD CAP (0% win rate above this in 20 real windows)
    Total:      600 shares, ${sum(p * 100 for p in [0.45, 0.50, 0.55, 0.60, 0.65, 0.70]):,.0f} deployed

  Hedge Side (smaller, opposite direction):
    Prices:     $0.45, $0.50
    Shares:     40 per level (30-40% of favored)
    Max price:  $0.55
    Total:      80 shares, ${sum(p * 40 for p in [0.45, 0.50]):,.0f} deployed

  Insurance (penny buys on losing side, after 50% of window):
    Prices:     $0.02, $0.05, $0.08
    Shares:     50 per level
    Max price:  $0.10 HARD CAP
    Total:      150 shares, ${sum(p * 50 for p in [0.02, 0.05, 0.08]):,.0f} deployed

  Hard Rules:
    Never fill above $0.75 on favored side
    Never fill above $0.55 on hedge side
    Never fill above $0.10 on insurance
    Max 3 side switches per window
    Stop all trading at 80% through window
    Signal delay: 15 seconds from window open

  MATH:
    Total deployed per window:    ${rec_pos.total_deployed:>8,.0f}
    PnL if correct (win):        ${rec_w:>+8,.0f}  ({rec_w/rec_pos.total_deployed*100:+.1f}%)
    PnL if wrong (loss):         ${rec_l:>+8,.0f}  ({rec_l/rec_pos.total_deployed*100:+.1f}%)
    Win/Loss ratio:              {abs(rec_w/rec_l):.2f}x
    Breakeven accuracy:          {rec_be:.1%}
    Recovery when wrong:         {rec_pos.recovery_if('Down')/rec_pos.total_deployed*100:.0f}%

  EXPECTED PERFORMANCE (at 55% signal accuracy):
    Per window:  ${0.55*rec_w + 0.45*rec_l:>+7,.1f}
    Per day (48 windows): ${(0.55*rec_w + 0.45*rec_l) * 48:>+7,.0f}
    Per month:   ${(0.55*rec_w + 0.45*rec_l) * 48 * 30:>+9,.0f}
    Starting capital needed: $3,000-5,000
""")
