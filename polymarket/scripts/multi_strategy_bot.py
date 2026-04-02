#!/usr/bin/env python3
"""
Multi-Strategy Paper Trading Bot — BTC 15m

Runs 7 different strategy configurations simultaneously on the same window.
Each strategy has independent position tracking and balance.
Produces a comparison table after each window.

Usage:
    python3 scripts/multi_strategy_bot.py [--spy WALLET]
"""

from __future__ import annotations

import argparse
import json
import logging
import os
import threading
import time
from copy import deepcopy
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, List, Optional, Tuple

import requests

# Import components from the main bot
import sys
sys.path.insert(0, os.path.dirname(__file__))
from btc_15m_bot import (
    CONFIG, Market, Fill, Position, WindowManager, MarketDiscovery,
    SignalEngine, PaperExecutor, OrderManager, SettlementHandler,
    WindowRecorder, SpyThread, GAMMA_API, CLOB_REST, HEISENBERG_API,
    log,
)

# ═══════════════════════════════════════════════════════════════════════════════
# STRATEGY DEFINITIONS
# ═══════════════════════════════════════════════════════════════════════════════

STRATEGIES = {
    "A_baseline": {
        "name": "A: Baseline (our current)",
        "description": "Fixed direction at +15s, penny insurance at 50%, stop at 80%",
        "signal_delay_sec": 15,
        "favored_prices": [0.45, 0.50, 0.55, 0.60, 0.65, 0.70],
        "favored_shares": 100,
        "favored_max_price": 0.75,
        "insurance_prices": [0.01, 0.02, 0.03, 0.05, 0.08],
        "insurance_shares": 100,
        "insurance_max_price": 0.10,
        "insurance_start_pct": 50,
        "stop_trading_pct": 80,
        "pivot_enabled": False,
    },
    "B_early_entry": {
        "name": "B: Early Entry (+5s signal)",
        "description": "Enter at +5s instead of +15s — faster but noisier signal",
        "signal_delay_sec": 5,
        "favored_prices": [0.45, 0.48, 0.50, 0.52, 0.55],
        "favored_shares": 120,
        "favored_max_price": 0.60,
        "insurance_prices": [0.01, 0.02, 0.05],
        "insurance_shares": 80,
        "insurance_max_price": 0.10,
        "insurance_start_pct": 50,
        "stop_trading_pct": 80,
        "pivot_enabled": False,
    },
    "C_late_confirm": {
        "name": "C: Late Confirmation (+45s)",
        "description": "Wait 45s for strong signal — fewer fills but higher accuracy",
        "signal_delay_sec": 45,
        "favored_prices": [0.55, 0.60, 0.65, 0.70],
        "favored_shares": 150,
        "favored_max_price": 0.75,
        "insurance_prices": [0.02, 0.05, 0.08],
        "insurance_shares": 100,
        "insurance_max_price": 0.10,
        "insurance_start_pct": 50,
        "stop_trading_pct": 80,
        "pivot_enabled": False,
    },
    "D_both_sides": {
        "name": "D: Both Sides (market maker)",
        "description": "Buy BOTH Up and Down at $0.45-$0.55 from the start",
        "signal_delay_sec": 10,
        "favored_prices": [0.45, 0.48, 0.50, 0.53, 0.55],
        "favored_shares": 100,
        "favored_max_price": 0.60,
        "insurance_prices": [0.45, 0.48, 0.50, 0.53, 0.55],  # same as favored = buy both
        "insurance_shares": 100,
        "insurance_max_price": 0.60,
        "insurance_start_pct": 0,  # insurance immediately (= both sides from start)
        "stop_trading_pct": 80,
        "pivot_enabled": False,
    },
    "E_pivot": {
        "name": "E: Mid-Window Pivot (Scallops-style)",
        "description": "Enter at +15s, re-check at 30%, pivot if BTC reversed",
        "signal_delay_sec": 15,
        "favored_prices": [0.45, 0.50, 0.55, 0.60, 0.65],
        "favored_shares": 80,
        "favored_max_price": 0.70,
        "insurance_prices": [0.01, 0.02, 0.05],
        "insurance_shares": 50,
        "insurance_max_price": 0.10,
        "insurance_start_pct": 60,
        "stop_trading_pct": 85,
        "pivot_enabled": True,
        "pivot_check_pct": 30,
        "pivot_prices": [0.45, 0.50, 0.55, 0.60, 0.65],
        "pivot_shares": 120,
    },
    "F_tight_cheap": {
        "name": "F: Tight & Cheap ($0.45-$0.55 only)",
        "description": "Only buy at $0.45-$0.55 — max edge per share, fewer fills",
        "signal_delay_sec": 15,
        "favored_prices": [0.45, 0.47, 0.49, 0.51, 0.53, 0.55],
        "favored_shares": 100,
        "favored_max_price": 0.55,
        "insurance_prices": [0.01, 0.02, 0.03],
        "insurance_shares": 80,
        "insurance_max_price": 0.05,
        "insurance_start_pct": 50,
        "stop_trading_pct": 75,
        "pivot_enabled": False,
    },
    "G_heavy_insurance": {
        "name": "G: Heavy Insurance (500 shares pennies)",
        "description": "Normal favored + massive penny insurance on losing side",
        "signal_delay_sec": 15,
        "favored_prices": [0.45, 0.50, 0.55, 0.60, 0.65, 0.70],
        "favored_shares": 80,
        "favored_max_price": 0.75,
        "insurance_prices": [0.01, 0.01, 0.02, 0.02, 0.03, 0.03, 0.05, 0.05, 0.08, 0.08],
        "insurance_shares": 100,
        "insurance_max_price": 0.10,
        "insurance_start_pct": 40,
        "stop_trading_pct": 80,
        "pivot_enabled": False,
    },
    "H_continuous_momentum": {
        "name": "H: Continuous Momentum",
        "description": "Re-checks BTC every 2min, adds to winning side, cuts losing side",
        "signal_delay_sec": 15,
        "favored_prices": [0.45, 0.50, 0.55],
        "favored_shares": 60,
        "favored_max_price": 0.60,
        "insurance_prices": [0.02, 0.05],
        "insurance_shares": 40,
        "insurance_max_price": 0.10,
        "insurance_start_pct": 999,  # don't auto-post insurance — dynamic handles it
        "stop_trading_pct": 85,
        "pivot_enabled": False,
        "continuous_enabled": True,
        "continuous_interval_pct": 15,  # re-check every 15% of window (~2.25min)
        "continuous_add_prices": [0.50, 0.55, 0.60],
        "continuous_add_shares": 50,
    },
    "I_adaptive_scallops": {
        "name": "I: Adaptive Scallops",
        "description": "Start small, monitor BTC continuously, scale into confirmed direction",
        "signal_delay_sec": 10,
        "favored_prices": [0.48, 0.50, 0.52],
        "favored_shares": 40,
        "favored_max_price": 0.65,
        "insurance_prices": [0.01, 0.03, 0.05],
        "insurance_shares": 60,
        "insurance_max_price": 0.10,
        "insurance_start_pct": 999,  # dynamic
        "stop_trading_pct": 85,
        "pivot_enabled": False,
        "continuous_enabled": True,
        "continuous_interval_pct": 10,  # re-check every 10% (~1.5min)
        "continuous_add_prices": [0.45, 0.50, 0.55, 0.60],
        "continuous_add_shares": 80,
    },
}


# ═══════════════════════════════════════════════════════════════════════════════
# STRATEGY RUNNER
# ═══════════════════════════════════════════════════════════════════════════════

@dataclass
class StrategyResult:
    name: str
    direction: str
    up_shares: float = 0.0
    up_cost: float = 0.0
    down_shares: float = 0.0
    down_cost: float = 0.0
    total_deployed: float = 0.0
    pnl: float = 0.0
    roi_pct: float = 0.0
    fills_count: int = 0
    pivoted: bool = False


class StrategyRunner:
    """Runs one strategy config against a window.

    Tracks position by TOKEN ID (not direction) to avoid the pivot re-attribution bug.
    When direction changes, fills from before the pivot keep their original side.
    """

    def __init__(self, key: str, strat_config: dict, market: Market,
                 signal: SignalEngine, base_config: dict):
        self.key = key
        self.strat = strat_config
        self.market = market
        self.signal = signal
        self.config = {**base_config, **strat_config}
        self.executor = PaperExecutor()
        self.direction: Optional[str] = None
        self.pivoted = False
        self.insurance_posted = False
        self.stopped = False

        # Track position by token_id directly — immune to direction changes
        self.up_shares: float = 0.0
        self.up_cost: float = 0.0
        self.down_shares: float = 0.0
        self.down_cost: float = 0.0
        self._processed_fills: set = set()  # (oid, ts) dedup
        self._order_token_map: Dict[str, str] = {}  # oid -> token_id
        self._last_continuous_pct: float = 0.0  # for continuous strategies

    def _place_order(self, token_id: str, price: float, shares: float) -> bool:
        """Place a GTC order and track which token it's for."""
        if price > self.config.get("favored_max_price", 0.75):
            return False
        projected = (self.up_cost + self.down_cost) + (shares * price)
        if projected > self.config.get("max_deploy_per_window", 500):
            return False
        oid = self.executor.place_gtc_order(token_id, "BUY", price, shares)
        self._order_token_map[oid] = token_id
        return True

    def _sync_fills(self):
        """Sync fills from executor into position, using token_id for side."""
        for oid, price, shares, ts in self.executor.get_fills():
            key = (oid, ts)
            if key in self._processed_fills:
                continue
            self._processed_fills.add(key)

            token_id = self._order_token_map.get(oid, "")
            usdc = shares * price
            if token_id == self.market.token_up:
                self.up_shares += shares
                self.up_cost += usdc
            elif token_id == self.market.token_down:
                self.down_shares += shares
                self.down_cost += usdc

    def enter(self):
        """Determine direction and post favored ladder."""
        d = self.signal.compute_direction()
        if d is None:
            return False
        self.direction = d

        if self.direction == "Up":
            favored_token = self.market.token_up
        else:
            favored_token = self.market.token_down

        shares = self.strat["favored_shares"]
        if self.config.get("mode") == "micro-live":
            shares = self.config.get("micro_live_size", 1.0)
        for price in self.strat["favored_prices"]:
            self._place_order(favored_token, price, shares)
        return True

    def tick(self, pct: float):
        """Called every 2s during the window."""
        if self.stopped:
            return

        self.executor.tick()
        self._sync_fills()

        # Pivot check (Strategy E)
        if (self.strat.get("pivot_enabled") and not self.pivoted
                and pct >= self.strat.get("pivot_check_pct", 30)):
            new_dir = self.signal.compute_direction()
            if new_dir and new_dir != self.direction:
                log.info(f"  [{self.key}] PIVOT: {self.direction} → {new_dir} at {pct:.0f}%")
                self.executor.cancel_all()
                self.direction = new_dir
                self.pivoted = True

                if self.direction == "Up":
                    token = self.market.token_up
                else:
                    token = self.market.token_down

                shares = self.strat.get("pivot_shares", self.strat["favored_shares"])
                for price in self.strat.get("pivot_prices", self.strat["favored_prices"]):
                    self._place_order(token, price, shares)

        # Continuous momentum check (Strategies H, I)
        if self.strat.get("continuous_enabled"):
            interval = self.strat.get("continuous_interval_pct", 15)
            if pct >= self._last_continuous_pct + interval and pct < self.strat["stop_trading_pct"]:
                self._last_continuous_pct = pct
                current_dir = self.signal.compute_direction()
                if current_dir:
                    if current_dir == self.direction:
                        # Confirmed — add more to the favored side
                        token = self.market.token_up if self.direction == "Up" else self.market.token_down
                        for price in self.strat.get("continuous_add_prices", []):
                            self._place_order(token, price, self.strat.get("continuous_add_shares", 50))
                        log.info(f"  [{self.key}] CONFIRM {current_dir} at {pct:.0f}% — adding more")
                    else:
                        # Reversed — cancel favored, post on new direction
                        self.executor.cancel_all()
                        self.direction = current_dir
                        self.pivoted = True
                        token = self.market.token_up if self.direction == "Up" else self.market.token_down
                        for price in self.strat.get("continuous_add_prices", []):
                            self._place_order(token, price, self.strat.get("continuous_add_shares", 50))
                        log.info(f"  [{self.key}] REVERSE → {current_dir} at {pct:.0f}% — switching sides")

        # Insurance
        if (not self.insurance_posted
                and pct >= self.strat["insurance_start_pct"]):
            if self.direction == "Up":
                ins_token = self.market.token_down
            else:
                ins_token = self.market.token_up

            ins_shares = self.strat["insurance_shares"]
            for price in self.strat["insurance_prices"]:
                if price <= self.strat["insurance_max_price"]:
                    oid = self.executor.place_gtc_order(ins_token, "BUY", price, ins_shares)
                    self._order_token_map[oid] = ins_token
            self.insurance_posted = True

        # Stop trading
        if not self.stopped and pct >= self.strat["stop_trading_pct"]:
            self.executor.cancel_all()
            self.stopped = True

    def result(self, winner: Optional[str]) -> StrategyResult:
        """Compute final result."""
        self.executor.tick()
        self._sync_fills()

        deployed = self.up_cost + self.down_cost
        if winner == "Up":
            pnl = self.up_shares - deployed
        elif winner == "Down":
            pnl = self.down_shares - deployed
        else:
            pnl = 0
        roi = pnl / deployed * 100 if deployed > 0 else 0

        return StrategyResult(
            name=self.strat["name"],
            direction=self.direction or "None",
            up_shares=self.up_shares,
            up_cost=self.position.up_cost,
            down_shares=self.position.down_shares,
            down_cost=self.position.down_cost,
            total_deployed=deployed,
            pnl=pnl,
            roi_pct=roi,
            fills_count=len(self._processed_fills),
            pivoted=self.pivoted,
        )

    def stop(self):
        self.executor.stop()


# ═══════════════════════════════════════════════════════════════════════════════
# DAILY TRACKER
# ═══════════════════════════════════════════════════════════════════════════════

class DailyTracker:
    def __init__(self):
        self.results: Dict[str, List[StrategyResult]] = {k: [] for k in STRATEGIES}
        self.windows: List[str] = []

    def add(self, window_slug: str, results: Dict[str, StrategyResult]):
        self.windows.append(window_slug)
        for key, result in results.items():
            self.results[key].append(result)

    def print_summary(self):
        print()
        print("=" * 130)
        print("DAILY STRATEGY COMPARISON")
        print("=" * 130)
        print()
        print(f"  {'Strategy':>40} | {'Windows':>7} | {'W/L':>7} | {'Total PnL':>10} | "
              f"{'Avg PnL':>8} | {'Avg ROI':>7} | {'Deployed':>9} | {'Fills':>6}")
        print("  " + "-" * 120)

        for key in STRATEGIES:
            results = self.results[key]
            if not results:
                continue
            wins = sum(1 for r in results if r.pnl > 0)
            losses = sum(1 for r in results if r.pnl < 0)
            total_pnl = sum(r.pnl for r in results)
            avg_pnl = total_pnl / len(results)
            total_dep = sum(r.total_deployed for r in results)
            avg_roi = total_pnl / total_dep * 100 if total_dep > 0 else 0
            total_fills = sum(r.fills_count for r in results)

            print(f"  {STRATEGIES[key]['name']:>40} | {len(results):>7} | "
                  f"{wins:>3}/{losses:<3} | ${total_pnl:>+8,.0f} | "
                  f"${avg_pnl:>+6,.0f} | {avg_roi:>+5.1f}% | ${total_dep:>8,.0f} | {total_fills:>6}")

    def print_window_comparison(self, window_slug: str, results: Dict[str, StrategyResult],
                                winner: str, spy_data: Optional[dict] = None):
        print()
        print(f"{'─' * 130}")
        print(f"  Window: {window_slug} | Winner: {winner}")
        print(f"{'─' * 130}")
        print(f"  {'Strategy':>40} | {'Dir':>5} | {'Pivot':>5} | {'Fills':>5} | "
              f"{'Deployed':>9} | {'PnL':>9} | {'ROI':>7} | {'Up Sh':>6} | {'Dn Sh':>6}")
        print("  " + "-" * 110)

        for key in STRATEGIES:
            r = results[key]
            pivot_str = "YES" if r.pivoted else ""
            print(f"  {r.name:>40} | {r.direction:>5} | {pivot_str:>5} | {r.fills_count:>5} | "
                  f"${r.total_deployed:>8,.0f} | ${r.pnl:>+8,.0f} | {r.roi_pct:>+5.1f}% | "
                  f"{r.up_shares:>5.0f} | {r.down_shares:>5.0f}")

        if spy_data:
            spy_up = spy_data.get("up_shares", 0)
            spy_dn = spy_data.get("down_shares", 0)
            spy_total = spy_data.get("total_deployed", 0)
            if winner == "Up":
                spy_pnl = spy_up - spy_total
            elif winner == "Down":
                spy_pnl = spy_dn - spy_total
            else:
                spy_pnl = 0
            spy_roi = spy_pnl / spy_total * 100 if spy_total > 0 else 0
            print(f"  {'>>> Scallops (spy)':>40} | {spy_data.get('direction', '?'):>5} | {'':>5} | "
                  f"{spy_data.get('trades_count', 0):>5} | ${spy_total:>8,.0f} | ${spy_pnl:>+8,.0f} | "
                  f"{spy_roi:>+5.1f}% | {spy_up:>5.0f} | {spy_dn:>5.0f}")


# ═══════════════════════════════════════════════════════════════════════════════
# MAIN
# ═══════════════════════════════════════════════════════════════════════════════

def main():
    parser = argparse.ArgumentParser(description="Multi-Strategy BTC 15m Paper Bot")
    parser.add_argument("--spy", type=str, default=None)
    args = parser.parse_args()

    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s [%(levelname)s] %(message)s",
        datefmt="%H:%M:%S",
        handlers=[
            logging.StreamHandler(),
            logging.FileHandler("multi_bot.log"),
        ],
    )

    log.info(f"Starting Multi-Strategy Bot | {len(STRATEGIES)} strategies")
    for key, s in STRATEGIES.items():
        log.info(f"  {s['name']}: {s['description']}")

    # Start Binance WS
    signal = SignalEngine(min_delta=5.0)  # lower threshold to catch more signals
    signal.start_ws()
    time.sleep(2)

    discovery = MarketDiscovery(asset="btc")
    wm = WindowManager(900)
    tracker = DailyTracker()
    replay_dir = Path("window_replay_multi/")
    replay_dir.mkdir(exist_ok=True)

    spy_wallet = args.spy or "0xe1d6b51521bd4365769199f392f9818661bd907c"
    spy_enabled = args.spy is not None

    try:
        while True:
            window_open = wm.wait_for_window()

            market = discovery.find_market(window_open)
            if market is None:
                log.warning("No market found. Skipping.")
                continue

            log.info(f"Window: {market.slug}")

            # Start spy
            spy = None
            if spy_enabled:
                spy = SpyThread(
                    wallet=spy_wallet,
                    api_key=CONFIG.get("heisenberg_api_key", ""),
                    window_open=window_open,
                    window_close=window_open + 900,
                    slug=market.slug,
                    poll_interval=5,
                )
                spy.start()

            # Wait for the LATEST signal delay (longest among strategies)
            max_delay = max(s["signal_delay_sec"] for s in STRATEGIES.values())

            # Snapshot open price
            signal.snapshot_open()

            # Create runners for each strategy
            runners: Dict[str, StrategyRunner] = {}
            for key, strat in STRATEGIES.items():
                runners[key] = StrategyRunner(
                    key=key, strat_config=strat, market=market,
                    signal=signal, base_config=CONFIG,
                )

            # Stagger entries by signal delay
            entered_at: Dict[str, bool] = {}
            entry_times = sorted(set(s["signal_delay_sec"] for s in STRATEGIES.values()))

            start_time = time.time()
            for delay in entry_times:
                # Wait until this delay has passed
                elapsed = time.time() - start_time
                wait = delay - elapsed
                if wait > 0:
                    time.sleep(wait)

                # Enter all strategies with this delay
                for key, runner in runners.items():
                    if runner.strat["signal_delay_sec"] == delay and key not in entered_at:
                        ok = runner.enter()
                        entered_at[key] = ok
                        if ok:
                            log.info(f"  [{key}] Entered {runner.direction} (delay={delay}s)")
                        else:
                            log.info(f"  [{key}] Skipped (no signal at {delay}s)")

            # Run through the window — tick all strategies every 2 seconds
            while True:
                pct = wm.pct_through()

                for key, runner in runners.items():
                    if key in entered_at and entered_at[key]:
                        runner.tick(pct)

                if pct >= 100:
                    break
                time.sleep(2)

            # Resolve winner — try settlement API, fall back to BTC price
            sh = SettlementHandler()
            winner = sh.resolve(
                slug=market.slug, condition_id=market.condition_id,
                market_id=market.market_id,
                token_up=market.token_up, token_down=market.token_down,
                retries=3, delay=5,
            )
            if winner is None:
                # Fallback: compare BTC price now vs open price
                btc_now = signal.current_price
                btc_open = signal.open_price
                if btc_now > btc_open:
                    winner = "Up"
                elif btc_now < btc_open:
                    winner = "Down"
                log.info(f"Settlement fallback: BTC ${btc_open:,.2f} → ${btc_now:,.2f} → {winner}")
            log.info(f"Winner: {winner}")

            # Collect results
            results: Dict[str, StrategyResult] = {}
            for key, runner in runners.items():
                results[key] = runner.result(winner)
                runner.stop()

            # Get spy data
            spy_data = None
            if spy:
                spy.stop()
                spy_data = spy.get_data()

            # Print comparison
            tracker.add(market.slug, results)
            tracker.print_window_comparison(market.slug, results, winner or "?", spy_data)
            tracker.print_summary()

            # Save replay
            replay = {
                "window": {
                    "slug": market.slug,
                    "open": datetime.fromtimestamp(market.window_open, tz=timezone.utc).isoformat(),
                    "close": datetime.fromtimestamp(market.window_close, tz=timezone.utc).isoformat(),
                    "winner": winner,
                },
                "strategies": {
                    key: {
                        "name": r.name, "direction": r.direction,
                        "up_shares": r.up_shares, "up_cost": round(r.up_cost, 2),
                        "down_shares": r.down_shares, "down_cost": round(r.down_cost, 2),
                        "deployed": round(r.total_deployed, 2),
                        "pnl": round(r.pnl, 2), "roi_pct": round(r.roi_pct, 1),
                        "fills": r.fills_count, "pivoted": r.pivoted,
                    }
                    for key, r in results.items()
                },
                "spy": spy_data,
            }
            open_dt = datetime.fromtimestamp(market.window_open, tz=timezone.utc)
            fname = f"{open_dt.strftime('%Y-%m-%d_%H-%M')}_multi.json"
            (replay_dir / fname).write_text(json.dumps(replay, indent=2, default=str))

            log.info(f"Replay: {replay_dir / fname}")

    except KeyboardInterrupt:
        log.info("Shutting down...")
    finally:
        signal.stop_ws()
        tracker.print_summary()
        log.info("Done.")


if __name__ == "__main__":
    main()
