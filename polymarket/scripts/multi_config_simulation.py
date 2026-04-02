#!/usr/bin/env python3
"""
Multi-Config Strategy Simulation — Run multiple parameter sets against real data.

Loads real window data from JSON, runs 3+ strategy configs, and produces
a complete comparison report.

Usage:
    python scripts/multi_config_simulation.py [--extra-data scripts/extra_windows.json]
"""

import json
import math
import random
import sys
from dataclasses import dataclass, field
from typing import List, Dict, Tuple, Optional
from pathlib import Path

random.seed(42)


# ═══════════════════════════════════════════════════════════════════════════════
# STRATEGY CONFIGS (TOML-like dicts)
# ═══════════════════════════════════════════════════════════════════════════════

CONFIGS = {
    "A_conservative": {
        "name": "A: Conservative (max $0.65)",
        "favored_prices": [0.45, 0.50, 0.55, 0.60, 0.65],
        "favored_shares": 100,
        "hedge_prices": [0.45, 0.50],
        "hedge_shares": 40,
        "insurance_prices": [0.02, 0.05, 0.08],
        "insurance_shares": 50,
        "max_price": 0.65,
        "description": "Tight price cap, never above $0.65. Maximum safety.",
    },
    "B_balanced": {
        "name": "B: Balanced (max $0.70)",
        "favored_prices": [0.45, 0.50, 0.55, 0.60, 0.65, 0.70],
        "favored_shares": 100,
        "hedge_prices": [0.45, 0.50],
        "hedge_shares": 40,
        "insurance_prices": [0.02, 0.05, 0.08],
        "insurance_shares": 50,
        "max_price": 0.70,
        "description": "Our recommended config from initial analysis.",
    },
    "C_aggressive": {
        "name": "C: Aggressive (max $0.75)",
        "favored_prices": [0.45, 0.50, 0.55, 0.60, 0.65, 0.70, 0.75],
        "favored_shares": 100,
        "hedge_prices": [0.45, 0.50],
        "hedge_shares": 40,
        "insurance_prices": [0.02, 0.05, 0.08],
        "insurance_shares": 50,
        "max_price": 0.75,
        "description": "Slightly wider cap. More fills but thinner edge per fill.",
    },
    "D_heavy_insurance": {
        "name": "D: Heavy Insurance (max $0.70, 3x insurance)",
        "favored_prices": [0.45, 0.50, 0.55, 0.60, 0.65, 0.70],
        "favored_shares": 100,
        "hedge_prices": [0.45],
        "hedge_shares": 30,
        "insurance_prices": [0.01, 0.02, 0.03, 0.05, 0.08, 0.10],
        "insurance_shares": 80,
        "max_price": 0.70,
        "description": "Minimal hedge, heavy penny insurance. Exploits cheap reversal protection.",
    },
    "E_no_hedge": {
        "name": "E: No Hedge (max $0.70, insurance only)",
        "favored_prices": [0.45, 0.50, 0.55, 0.60, 0.65, 0.70],
        "favored_shares": 100,
        "hedge_prices": [],
        "hedge_shares": 0,
        "insurance_prices": [0.01, 0.02, 0.03, 0.05, 0.08],
        "insurance_shares": 100,
        "max_price": 0.70,
        "description": "No mid-price hedge. All protection via penny insurance.",
    },
    "F_scallops_style": {
        "name": "F: Scallops-Style (chase to $0.95)",
        "favored_prices": [0.50, 0.55, 0.60, 0.65, 0.70, 0.75, 0.80, 0.85, 0.90, 0.95],
        "favored_shares": 100,
        "hedge_prices": [0.45, 0.50],
        "hedge_shares": 40,
        "insurance_prices": [0.02, 0.05, 0.08],
        "insurance_shares": 50,
        "max_price": 0.95,
        "description": "Baseline: what Scallops does. Chase to $0.95.",
    },
    "G_tight_spread": {
        "name": "G: Tight Spread ($0.48-$0.58 only)",
        "favored_prices": [0.48, 0.50, 0.52, 0.54, 0.56, 0.58],
        "favored_shares": 120,
        "hedge_prices": [0.48, 0.50],
        "hedge_shares": 50,
        "insurance_prices": [0.02, 0.05],
        "insurance_shares": 60,
        "max_price": 0.60,
        "description": "Ultra-tight range near fair value. Max fill rate, max edge per fill.",
    },
}


# ═══════════════════════════════════════════════════════════════════════════════
# REAL DATA — 20 windows from our analysis (08:45-15:30 UTC)
# Format: (time, winner, w_avg, l_avg, w_cost, l_cost, w_shares, scallops_pnl)
# ═══════════════════════════════════════════════════════════════════════════════

WINDOWS_BATCH1 = [
    ("08:45", "Down", 0.678, 0.348, 3463, 1511, 5107, 455),
    ("09:00", "Down", 0.569, 0.196, 3060, 1189, 5378, 1164),
    ("09:15", "Up", 0.945, 0.176, 10647, 779, 11267, -1451),
    ("09:30", "Down", 0.619, 0.360, 4382, 2359, 7080, 346),
    ("09:45", "Up", 0.555, 0.461, 2235, 2235, 4026, -479),
    ("10:30", "Up", 0.879, 0.248, 2586, 1004, 2942, -661),
    ("10:45", "Down", 0.652, 0.305, 1934, 829, 2967, 218),
    ("11:30", "Down", 0.630, 0.041, 1550, 82, 2460, 821),
    ("11:45", "Up", 0.301, 0.642, 831, 1930, 2761, -239),
    ("12:00", "Down", 0.567, 0.331, 4056, 2452, 7150, 648),
    ("12:15", "Down", 0.533, 0.186, 2464, 733, 4624, 1427),
    ("12:30", "Up", 0.963, 0.204, 6540, 1605, 6789, -1356),
    ("12:45", "Up", 0.142, 0.547, 341, 841, 2403, 1220),
    ("13:00", "Down", 0.538, 0.429, 3123, 2179, 5800, 498),
    ("13:15", "Up", 0.812, 0.394, 11052, 6216, 13611, -3656),
    ("14:00", "Up", 0.633, 0.365, 3541, 2468, 5593, -416),
    ("14:15", "Down", 0.661, 0.297, 2918, 1124, 4412, 370),
    ("14:45", "Down", 0.909, 0.224, 4472, 1363, 4917, -917),
    ("15:00", "Up", 0.568, 0.506, 3086, 2343, 5429, 655),
    ("15:15", "Down", 0.717, 0.099, 2703, 269, 3771, 783),
]


# ═══════════════════════════════════════════════════════════════════════════════
# SIMULATION ENGINE
# ═══════════════════════════════════════════════════════════════════════════════

def theoretical_pnl(config: dict) -> Tuple[float, float, float]:
    """Compute theoretical PnL (100% fill) for a config. Returns (win_pnl, loss_pnl, deployed)."""
    favored_cost = sum(p * config["favored_shares"] for p in config["favored_prices"])
    favored_shares = len(config["favored_prices"]) * config["favored_shares"]
    hedge_cost = sum(p * config["hedge_shares"] for p in config["hedge_prices"])
    hedge_shares = len(config["hedge_prices"]) * config["hedge_shares"]
    ins_cost = sum(p * config["insurance_shares"] for p in config["insurance_prices"])
    ins_shares = len(config["insurance_prices"]) * config["insurance_shares"]

    deployed = favored_cost + hedge_cost + ins_cost
    win_pnl = favored_shares - favored_cost - hedge_cost - ins_cost
    loss_pnl = (hedge_shares + ins_shares) - hedge_cost - ins_cost - favored_cost

    return win_pnl, loss_pnl, deployed


def breakeven_accuracy(win_pnl, loss_pnl) -> float:
    """Minimum accuracy for positive expectation."""
    if win_pnl + abs(loss_pnl) == 0:
        return 1.0
    return abs(loss_pnl) / (win_pnl + abs(loss_pnl))


def replay_on_real_data(config: dict, windows: list) -> dict:
    """Replay a config on real window data.

    For each window, we check: would this config have traded (w_avg <= max_price)?
    If yes, use the window's actual PnL direction but scaled to our config's theoretical PnL.
    If no, skip (PnL = 0).
    """
    max_price = config["max_price"]
    win_pnl, loss_pnl, deployed = theoretical_pnl(config)

    total_pnl = 0
    wins = 0
    losses = 0
    skipped = 0
    traded_windows = []

    for w in windows:
        time_str, winner, w_avg = w[0], w[1], w[2]
        sc_pnl = w[7]

        # Would we have traded? Check if the window's avg price is within our range
        if w_avg > max_price:
            skipped += 1
            traded_windows.append((time_str, "SKIP", 0, w_avg, sc_pnl))
            continue

        # Did Scallops win or lose?
        if sc_pnl > 0:
            pnl = win_pnl
            wins += 1
        else:
            pnl = loss_pnl
            losses += 1

        total_pnl += pnl
        traded_windows.append((time_str, "WIN" if pnl > 0 else "LOSS", pnl, w_avg, sc_pnl))

    return {
        "total_pnl": total_pnl,
        "wins": wins,
        "losses": losses,
        "skipped": skipped,
        "traded": wins + losses,
        "win_rate": wins / (wins + losses) * 100 if (wins + losses) > 0 else 0,
        "avg_pnl": total_pnl / (wins + losses) if (wins + losses) > 0 else 0,
        "windows": traded_windows,
    }


def monte_carlo(config: dict, accuracy: float, n_windows: int = 10000) -> dict:
    """Monte Carlo with theoretical PnL."""
    win_pnl, loss_pnl, deployed = theoretical_pnl(config)

    total_pnl = 0
    running = 0
    peak = 0
    max_dd = 0
    pnls = []

    for _ in range(n_windows):
        if random.random() < accuracy:
            pnl = win_pnl
        else:
            pnl = loss_pnl
        total_pnl += pnl
        running += pnl
        peak = max(peak, running)
        max_dd = max(max_dd, peak - running)
        pnls.append(pnl)

    mean = sum(pnls) / len(pnls)
    var = sum((p - mean) ** 2 for p in pnls) / len(pnls)
    std = math.sqrt(var) if var > 0 else 0.001
    sharpe = mean / std

    return {
        "total_pnl": total_pnl,
        "avg_pnl": mean,
        "deployed": deployed,
        "roi": mean / deployed * 100 if deployed > 0 else 0,
        "max_dd": max_dd,
        "sharpe": sharpe,
    }


def bankroll_sim(config: dict, accuracy: float, starting: float = 5000,
                 n_windows: int = 500, n_sims: int = 3000) -> dict:
    """Bankroll simulation."""
    win_pnl, loss_pnl, deployed = theoretical_pnl(config)

    finals = []
    mins = []
    ruins = 0

    for _ in range(n_sims):
        br = starting
        min_br = br
        for _ in range(n_windows):
            if br < deployed:
                ruins += 1
                break
            br += win_pnl if random.random() < accuracy else loss_pnl
            min_br = min(min_br, br)
        finals.append(br)
        mins.append(min_br)

    finals.sort()
    mins.sort()
    n = len(finals)

    return {
        "p5": finals[int(n * 0.05)],
        "p25": finals[int(n * 0.25)],
        "median": finals[int(n * 0.50)],
        "p75": finals[int(n * 0.75)],
        "p95": finals[int(n * 0.95)],
        "worst_dd": starting - mins[int(n * 0.05)],
        "median_dd": starting - mins[int(n * 0.50)],
        "ruin_pct": ruins / n_sims * 100,
    }


# ═══════════════════════════════════════════════════════════════════════════════
# LOAD EXTRA DATA
# ═══════════════════════════════════════════════════════════════════════════════

extra_data_path = Path("/Users/milerius/Documents/Mantis/polymarket/scripts/extra_windows.json")
all_windows = list(WINDOWS_BATCH1)

if extra_data_path.exists():
    with open(extra_data_path) as f:
        extra = json.load(f)
    for w in extra:
        # Extra data has: time_str, winner, w_avg, l_avg, total, pnl
        # Reconstruct w_cost, l_cost, w_shares from what we have
        total = w.get("total", 0)
        w_avg = w.get("w_avg", 0)
        l_avg = w.get("l_avg", 0)
        pnl = w.get("pnl", 0)
        dom_pct = w.get("dom_pct", 50) / 100

        # Estimate w_cost and l_cost from total and dom_pct
        w_cost = total * dom_pct
        l_cost = total * (1 - dom_pct)
        # Estimate w_shares: pnl = w_shares - w_cost - l_cost => w_shares = pnl + total
        w_shares = pnl + total if pnl > -total else 0

        all_windows.append((
            w.get("time_str", "??:??"),
            w.get("winner", "?"),
            w_avg,
            l_avg,
            w_cost,
            l_cost,
            w_shares,
            pnl,
        ))
    print(f"Loaded {len(extra)} extra windows from {extra_data_path}")
else:
    print(f"No extra data at {extra_data_path}, using {len(all_windows)} windows only")

# Filter to measurable windows (winner known, non-zero deployed)
measurable = [w for w in all_windows if w[1] in ("Up", "Down") and (w[4] + w[5]) > 0]
print(f"Total windows: {len(all_windows)} | Measurable: {len(measurable)}")


# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 1: THEORETICAL COMPARISON
# ═══════════════════════════════════════════════════════════════════════════════

print("\n" + "=" * 130)
print("SECTION 1: THEORETICAL P&L PER CONFIG (100% fill assumption)")
print("=" * 130)

print(f"\n  {'Config':>40} | {'Win PnL':>8} | {'Loss PnL':>9} | {'Deployed':>9} | {'Win ROI':>8} | {'Loss ROI':>9} | {'BE Acc':>6} | {'W/L Ratio':>9}")
print("  " + "-" * 115)

for key, cfg in CONFIGS.items():
    wp, lp, dep = theoretical_pnl(cfg)
    be = breakeven_accuracy(wp, lp)
    wr = abs(wp / lp) if lp != 0 else float('inf')
    print(f"  {cfg['name']:>40} | ${wp:>+6,.0f} | ${lp:>+7,.0f} | ${dep:>8,.0f} | "
          f"{wp/dep*100:>+6.1f}% | {lp/dep*100:>+7.1f}% | {be:>5.1%} | {wr:>8.2f}x")


# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 2: REPLAY ON REAL DATA
# ═══════════════════════════════════════════════════════════════════════════════

print("\n\n" + "=" * 130)
print(f"SECTION 2: REPLAY ON {len(measurable)} REAL WINDOWS")
print("=" * 130)

print(f"\n  {'Config':>40} | {'Traded':>7} | {'Skip':>5} | {'W/L':>7} | {'WR%':>5} | {'Total PnL':>10} | {'Avg PnL':>8} | {'SC PnL':>9}")
print("  " + "-" * 105)

# Also compute Scallops' actual PnL on measurable windows
sc_total = sum(w[7] for w in measurable)

for key, cfg in CONFIGS.items():
    result = replay_on_real_data(cfg, measurable)
    print(f"  {cfg['name']:>40} | {result['traded']:>7} | {result['skipped']:>5} | "
          f"{result['wins']:>3}/{result['losses']:>3} | {result['win_rate']:>4.0f}% | "
          f"${result['total_pnl']:>+9,.0f} | ${result['avg_pnl']:>+6,.0f} | ${sc_total:>+8,.0f}")

print(f"\n  Scallops actual PnL across same {len(measurable)} windows: ${sc_total:>+,.0f}")


# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 3: MONTE CARLO AT DIFFERENT ACCURACY LEVELS
# ═══════════════════════════════════════════════════════════════════════════════

print("\n\n" + "=" * 130)
print("SECTION 3: MONTE CARLO (10,000 windows per config per accuracy)")
print("=" * 130)

for acc in [0.50, 0.55, 0.60]:
    print(f"\n  --- Signal Accuracy: {acc:.0%} ---")
    print(f"  {'Config':>40} | {'Avg PnL':>8} | {'ROI':>7} | {'MaxDD':>9} | {'Sharpe':>7} | {'Daily(48w)':>11} | {'Monthly':>10}")
    print("  " + "-" * 105)

    for key, cfg in CONFIGS.items():
        mc = monte_carlo(cfg, acc)
        daily = mc["avg_pnl"] * 48
        monthly = daily * 30
        print(f"  {cfg['name']:>40} | ${mc['avg_pnl']:>+6,.1f} | {mc['roi']:>+5.1f}% | "
              f"${mc['max_dd']:>8,.0f} | {mc['sharpe']:>6.3f} | ${daily:>+10,.0f} | ${monthly:>+9,.0f}")


# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 4: BANKROLL SIMULATION ($5K starting, 500 windows = ~5 days)
# ═══════════════════════════════════════════════════════════════════════════════

print("\n\n" + "=" * 130)
print("SECTION 4: BANKROLL SIMULATION ($5,000 start, 500 windows, 3,000 sims)")
print("=" * 130)

for acc in [0.52, 0.55, 0.60]:
    print(f"\n  --- Accuracy: {acc:.0%} ---")
    print(f"  {'Config':>40} | {'P5':>9} | {'Median':>9} | {'P95':>9} | {'Worst DD':>9} | {'Ruin%':>6}")
    print("  " + "-" * 95)

    for key, cfg in CONFIGS.items():
        br = bankroll_sim(cfg, acc)
        print(f"  {cfg['name']:>40} | ${br['p5']:>8,.0f} | ${br['median']:>8,.0f} | ${br['p95']:>8,.0f} | "
              f"${br['worst_dd']:>8,.0f} | {br['ruin_pct']:>4.1f}%")


# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 5: DETAILED WINDOW-BY-WINDOW for top 3 configs
# ═══════════════════════════════════════════════════════════════════════════════

print("\n\n" + "=" * 130)
print("SECTION 5: WINDOW-BY-WINDOW REPLAY (top 3 configs)")
print("=" * 130)

top3 = ["B_balanced", "D_heavy_insurance", "G_tight_spread"]

for key in top3:
    cfg = CONFIGS[key]
    result = replay_on_real_data(cfg, measurable)
    wp, lp, dep = theoretical_pnl(cfg)

    print(f"\n  --- {cfg['name']} ---")
    print(f"  {cfg['description']}")
    print(f"  Theoretical: win=${wp:+,.0f} loss=${lp:+,.0f} deployed=${dep:,.0f}")
    print()
    print(f"  {'Time':>7} | {'Action':>6} | {'PnL':>8} | {'W.Avg':>6} | {'SC PnL':>8} | {'Better?':>8}")
    print("  " + "-" * 60)

    running = 0
    sc_running = 0
    for time_str, action, pnl, w_avg, sc_pnl in result["windows"]:
        running += pnl
        sc_running += sc_pnl
        better = "YES" if (pnl > sc_pnl) or (action == "SKIP" and sc_pnl < 0) else "no"
        w_avg_str = f"${w_avg:.3f}" if w_avg > 0 else "  n/a"
        print(f"  {time_str:>7} | {action:>6} | ${pnl:>+6,.0f} | {w_avg_str:>6} | ${sc_pnl:>+6,.0f} | {better:>8}")

    print(f"  {'TOTAL':>7} | {'':>6} | ${running:>+6,.0f} | {'':>6} | ${sc_running:>+6,.0f} | {'YES' if running > sc_running else 'no':>8}")


# ═══════════════════════════════════════════════════════════════════════════════
# SECTION 6: FINAL RECOMMENDATION
# ═══════════════════════════════════════════════════════════════════════════════

print("\n\n" + "=" * 130)
print("SECTION 6: FINAL RECOMMENDATION")
print("=" * 130)

# Score each config: combine replay PnL, breakeven, sharpe, bankroll safety
print(f"\n  Scoring: 40% real replay PnL + 30% breakeven ease + 20% Sharpe@55% + 10% ruin safety\n")

scores = {}
for key, cfg in CONFIGS.items():
    replay = replay_on_real_data(cfg, measurable)
    wp, lp, dep = theoretical_pnl(cfg)
    be = breakeven_accuracy(wp, lp)
    mc55 = monte_carlo(cfg, 0.55)
    br55 = bankroll_sim(cfg, 0.55)

    # Normalize scores (0-100)
    replay_score = min(100, max(0, (replay["total_pnl"] + 5000) / 100))  # -5K=0, +5K=100
    be_score = max(0, (1 - be) * 200)  # lower BE = better
    sharpe_score = min(100, max(0, mc55["sharpe"] * 100))
    ruin_score = max(0, 100 - br55["ruin_pct"] * 10)

    total_score = replay_score * 0.4 + be_score * 0.3 + sharpe_score * 0.2 + ruin_score * 0.1

    scores[key] = {
        "name": cfg["name"],
        "total": total_score,
        "replay": replay_score,
        "be": be_score,
        "sharpe": sharpe_score,
        "ruin": ruin_score,
        "replay_pnl": replay["total_pnl"],
        "be_acc": be,
        "sharpe_val": mc55["sharpe"],
    }

# Sort by total score
ranked = sorted(scores.items(), key=lambda x: -x[1]["total"])

print(f"  {'Rank':>5} | {'Config':>40} | {'Score':>6} | {'Replay':>7} | {'BE':>5} | {'Sharpe':>7} | {'Safe':>5} | {'Key Metrics'}")
print("  " + "-" * 110)

for rank, (key, s) in enumerate(ranked, 1):
    medal = {1: ">>>", 2: " >>", 3: "  >"}.get(rank, "   ")
    print(f"  {medal}{rank:>2} | {s['name']:>40} | {s['total']:>5.1f} | {s['replay']:>6.1f} | "
          f"{s['be']:>4.0f} | {s['sharpe']:>6.1f} | {s['ruin']:>4.0f} | "
          f"PnL=${s['replay_pnl']:>+,.0f} BE={s['be_acc']:.0%} Sh={s['sharpe_val']:.2f}")

winner_key = ranked[0][0]
winner = CONFIGS[winner_key]
print(f"\n  RECOMMENDED: {winner['name']}")
print(f"  {winner['description']}")
print(f"\n  Config:")
print(f"    Favored: {winner['favored_prices']} × {winner['favored_shares']} shares")
print(f"    Hedge:   {winner['hedge_prices']} × {winner['hedge_shares']} shares")
print(f"    Insurance: {winner['insurance_prices']} × {winner['insurance_shares']} shares")
print(f"    Max price: ${winner['max_price']}")
