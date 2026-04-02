#!/usr/bin/env python3
from __future__ import annotations

"""
BTC 15-minute Up/Down trading bot for Polymarket.

Tasks completed:
  Task 1: CONFIG, data classes, logger setup   [DONE]
  Task 2: WindowManager                        [TODO]
  Task 3: MarketDiscovery                      [DONE]
  Task 4: SignalEngine                         [TODO]
  Task 5: PaperExecutor                        [TODO]
  Task 6: LiveExecutor + MicroLiveExecutor     [TODO]
  Task 7: OrderManager + Safety Rules          [TODO]
  Task 8: SettlementHandler + WindowRecorder   [TODO]
  Task 9: SpyThread                            [TODO]
  Task 10: Main Loop + CLI                     [TODO]
"""

import json
import logging
import os
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Dict, List, Optional

import requests

# ---------------------------------------------------------------------------
# CONFIG
# ---------------------------------------------------------------------------

CONFIG = {
    "mode": "paper",
    "micro_live_size": 1.0,
    "asset": "btc",
    "timeframe": "15m",
    "window_duration": 900,
    "signal_delay_sec": 15,
    "min_btc_delta_usd": 10.0,
    "favored_prices": [0.45, 0.50, 0.55, 0.60, 0.65, 0.70],
    "favored_shares": 100,
    "favored_max_price": 0.75,
    "insurance_prices": [0.01, 0.02, 0.03, 0.05, 0.08],
    "insurance_shares": 100,
    "insurance_max_price": 0.10,
    "insurance_start_pct": 50,
    "stop_trading_pct": 80,
    "max_side_switches": 3,
    "max_deploy_per_window": 500,
    "max_daily_loss": 1000,
    "max_consecutive_losses": 8,
    "spy_enabled": False,
    "spy_wallet": "0xe1d6b51521bd4365769199f392f9818661bd907c",
    "spy_poll_interval_sec": 5,
    "heisenberg_api_key": os.environ.get("HEISENBERG_API_KEY", ""),
    "replay_dir": "window_replay/",
    "log_file": "bot.log",
}

# ---------------------------------------------------------------------------
# Logger
# ---------------------------------------------------------------------------

log = logging.getLogger("btc15m")

# ---------------------------------------------------------------------------
# Data Classes
# ---------------------------------------------------------------------------


@dataclass
class Market:
    """Represents a single BTC Up/Down 15-minute Polymarket window."""
    condition_id: str
    token_up: str
    token_down: str
    slug: str
    window_open: int    # Unix timestamp (seconds)
    window_close: int   # Unix timestamp (seconds)


@dataclass
class Fill:
    """A single order fill record."""
    ts: float
    side: str           # "Up" or "Down"
    price: float        # Price paid per share (0-1)
    shares: float       # Number of shares
    usdc: float         # USDC spent
    order_type: str     # "favored" or "insurance"
    order_id: str


class Position:
    """
    Tracks the current position within a single window.

    Maintains separate cost and share counts for the Up and Down sides,
    and provides PnL calculations assuming binary settlement (winner pays 1.00,
    loser pays 0.00).
    """

    def __init__(self) -> None:
        self.up_shares: float = 0.0
        self.up_cost: float = 0.0
        self.down_shares: float = 0.0
        self.down_cost: float = 0.0
        self.fills: List[Fill] = []

    def add_fill(self, side: str, shares: float, price: float, usdc: float,
                 ts: float = 0.0, order_type: str = "", order_id: str = "") -> None:
        """Record a fill for the given side."""
        if side == "Up":
            self.up_shares += shares
            self.up_cost += usdc
        elif side == "Down":
            self.down_shares += shares
            self.down_cost += usdc
        else:
            raise ValueError(f"Unknown side: {side!r}")
        self.fills.append(Fill(ts=ts, side=side, price=price, shares=shares, usdc=usdc,
                               order_type=order_type, order_id=order_id))

    @property
    def total_deployed(self) -> float:
        """Total USDC deployed across both sides."""
        return self.up_cost + self.down_cost

    def pnl_if(self, winning_side: str) -> float:
        """
        Calculate PnL assuming `winning_side` wins (pays 1.00 per share).
        The losing side pays 0.00.

        pnl = winning_shares * 1.00 - total_deployed
        """
        if winning_side == "Up":
            return self.up_shares * 1.0 - self.total_deployed
        elif winning_side == "Down":
            return self.down_shares * 1.0 - self.total_deployed
        else:
            raise ValueError(f"Unknown winning_side: {winning_side!r}")

    def recovery_pct(self, winning_side: str) -> float:
        """
        Percentage of deployed capital recovered if `winning_side` wins.
        Returns 0.0 if nothing has been deployed.
        """
        if self.total_deployed == 0.0:
            return 0.0
        if winning_side == "Up":
            return (self.up_shares / self.total_deployed) * 100.0
        elif winning_side == "Down":
            return (self.down_shares / self.total_deployed) * 100.0
        else:
            raise ValueError(f"Unknown winning_side: {winning_side!r}")


class WindowManager:
    def __init__(self, window_duration: int = 900):
        self.duration = window_duration
        self.window_open: int = 0
        self.window_close: int = 0

    def next_window_open(self, now: Optional[float] = None) -> int:
        now = int(time.time() if now is None else now)
        current_boundary = (now // self.duration) * self.duration
        pct = (now - current_boundary) / self.duration * 100
        if pct <= 5:
            return current_boundary
        else:
            return current_boundary + self.duration

    def set_window(self, open_ts: int):
        self.window_open = open_ts
        self.window_close = open_ts + self.duration

    def pct_through(self, now: Optional[float] = None) -> float:
        now = time.time() if now is None else now
        if self.window_open == 0:
            return 0.0
        elapsed = now - self.window_open
        return max(0.0, min(100.0, elapsed / self.duration * 100))

    def wait_for_window(self) -> int:
        """Sleep until next window open. Returns the window_open timestamp."""
        target = self.next_window_open()
        now = time.time()
        wait = target - now
        if wait > 0:
            log.info(f"Waiting {wait:.0f}s for next window at "
                     f"{datetime.fromtimestamp(target, tz=timezone.utc).strftime('%H:%M:%S')} UTC")
            time.sleep(wait)
        self.set_window(target)
        return target


# ---------------------------------------------------------------------------
# MarketDiscovery
# ---------------------------------------------------------------------------

GAMMA_API = "https://gamma-api.polymarket.com"


class MarketDiscovery:
    def __init__(self, asset: str = "btc"):
        self.asset = asset.lower()
        self._cache: Dict[int, Optional[Market]] = {}

    def find_market(self, window_open: int) -> Optional[Market]:
        if window_open in self._cache:
            return self._cache[window_open]

        try:
            resp = requests.get(
                f"{GAMMA_API}/events",
                params={
                    "limit": "100",
                    "active": "true",
                    "closed": "false",
                    "tag_slug": ["up-or-down", "15M"],
                },
                timeout=10,
            )
            resp.raise_for_status()
            events = resp.json()
        except (requests.exceptions.RequestException, ValueError) as e:
            log.warning(f"Gamma API error: {e}")
            return None

        window_close = window_open + CONFIG["window_duration"]
        target_slug_part = f"{self.asset}-updown-15m-{window_open}"

        for event in events:
            slug = event.get("slug", "").lower()
            if target_slug_part not in slug:
                continue

            for mkt in event.get("markets", []):
                if not mkt.get("active") or mkt.get("closed"):
                    continue

                try:
                    clob_ids = json.loads(mkt.get("clobTokenIds", "[]"))
                    outcomes = json.loads(mkt.get("outcomes", "[]"))
                except (json.JSONDecodeError, TypeError):
                    continue

                if len(clob_ids) < 2 or len(outcomes) < 2:
                    continue

                token_up, token_down = "", ""
                for i, outcome in enumerate(outcomes):
                    if outcome.lower() == "up":
                        token_up = clob_ids[i]
                    elif outcome.lower() == "down":
                        token_down = clob_ids[i]

                if not token_up or not token_down:
                    continue

                market = Market(
                    condition_id=mkt.get("conditionId", ""),
                    token_up=token_up,
                    token_down=token_down,
                    slug=event.get("slug", ""),
                    window_open=window_open,
                    window_close=window_close,
                )
                self._cache[window_open] = market
                return market

        self._cache[window_open] = None
        return None


# ---------------------------------------------------------------------------
# Placeholder — subsequent tasks will add:
#   Task 4:  SignalEngine
#   Task 5:  PaperExecutor
#   Task 6:  LiveExecutor / MicroLiveExecutor
#   Task 7:  OrderManager + safety rules
#   Task 8:  SettlementHandler + WindowRecorder
#   Task 9:  SpyThread
#   Task 10: main() + CLI entry point
# ---------------------------------------------------------------------------
