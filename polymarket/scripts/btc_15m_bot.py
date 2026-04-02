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
  Task 10: Main Loop + CLI                     [DONE]
"""

import argparse
import json
import logging
import os
import threading
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, List, Optional, Tuple

import requests

try:
    from py_clob_client.client import ClobClient
    from py_clob_client.clob_types import OrderArgs, OrderType
    from py_clob_client.order_builder.constants import BUY
    HAS_CLOB_CLIENT = True
except ImportError:
    HAS_CLOB_CLIENT = False
    ClobClient = None
    OrderArgs = None
    OrderType = None
    BUY = None

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
    "heisenberg_api_key": os.environ.get("HEISENBERG_API_KEY", "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJ0b2tlbl90eXBlIjoiYWNjZXNzIiwiZXhwIjoxNzgwMzM3ODU0LCJpYXQiOjE3NzUxNTM4NTQsImp0aSI6IjFkNTFlODk0YjgxNTRmNWE4ZDA5NTlmYzA2OTJiODhkIiwidXNlcl9pZCI6MTI0OCwic2NvcGUiOiJsYXVuY2hwYWQ6YWdlbnQtcmVhZCxyZXRyaWV2ZXI6ZWNoby1nZW5lcmF0aW9uLHJldHJpZXZlcjpmZWF0dXJlLWV4dHJhY3Rpb24sdXNlcjpyZWFkLHJldHJpZXZlcjphZ2VudC1vcHRpb24tcmV0cmlldmFsLGxhdW5jaHBhZDphZ2VudC1jcmVhdGlvbixsYXVuY2hwYWQ6YWdlbnQtdXBkYXRlLHVzZXI6d3JpdGUscmV0cmlldmVyOnNlbWFudGljLXJldHJpZXZhbCxsYXVuY2hwYWQ6ZWNoby1zdHlsZS1jcmVhdGlvbiIsInRva2VuX25hbWUiOiJiYXNlX2xvZ2luIn0.4KYt2DlYN_RNraB1PgrAaEA-3wJy0gMZMRAIrpDT4Dk"),
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
    market_id: Optional[int] = None  # Gamma API numeric ID


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
                    market_id=mkt.get("id"),
                )
                self._cache[window_open] = market
                return market

        self._cache[window_open] = None
        return None


# ---------------------------------------------------------------------------
# SignalEngine
# ---------------------------------------------------------------------------

BINANCE_WS_URL = "wss://stream.binance.com:9443/ws/btcusdt@trade"


class SignalEngine:
    def __init__(self, min_delta: float = 10.0):
        self.min_delta = min_delta
        self.open_price: float = 0.0
        self.current_price: float = 0.0
        self._ws_thread: Optional[threading.Thread] = None
        self._running = False
        self._lock = threading.Lock()

    def delta(self) -> float:
        with self._lock:
            return self.current_price - self.open_price

    def compute_direction(self) -> Optional[str]:
        d = self.delta()
        if abs(d) < self.min_delta:
            return None
        return "Up" if d > 0 else "Down"

    def snapshot_open(self):
        with self._lock:
            self.open_price = self.current_price
        log.info(f"Signal: open_price = ${self.open_price:,.2f}")

    def start_ws(self):
        self._running = True
        self._ws_thread = threading.Thread(target=self._ws_loop, daemon=True)
        self._ws_thread.start()

    def stop_ws(self):
        self._running = False
        if self._ws_thread is not None:
            self._ws_thread.join(timeout=3)

    def _ws_loop(self):
        import websockets.sync.client as ws_sync
        while self._running:
            try:
                with ws_sync.connect(BINANCE_WS_URL) as ws:
                    log.info("Binance WS connected")
                    while self._running:
                        msg = ws.recv(timeout=5)
                        data = json.loads(msg)
                        if "p" not in data:
                            continue
                        with self._lock:
                            self.current_price = float(data["p"])
            except Exception as e:
                if self._running:
                    log.warning(f"Binance WS error: {e}, reconnecting in 2s")
                    time.sleep(2)

    def wait_for_signal(self, delay_sec: float) -> Optional[str]:
        self.snapshot_open()
        time.sleep(delay_sec)
        direction = self.compute_direction()
        d = self.delta()
        log.info(f"Signal: BTC ${self.open_price:,.2f} → ${self.current_price:,.2f} "
                 f"(delta ${d:+,.2f}) → direction={direction}")
        return direction


# ---------------------------------------------------------------------------
# PaperExecutor
# ---------------------------------------------------------------------------

CLOB_REST = "https://clob.polymarket.com"


PM_WS_URL = "wss://ws-subscriptions-clob.polymarket.com/ws/market"


class PaperExecutor:
    """Paper executor with real-time Polymarket WebSocket for realistic fill simulation.

    At placement: sweeps all ask levels up to our price for immediate fills.
    For resting orders: subscribes to the Polymarket market WS and fills when
    the best_ask drops to our resting price.
    """

    def __init__(self):
        self._orders: Dict[str, dict] = {}
        self._fills: List[Tuple[str, float, float, float]] = []
        self._next_id = 0
        self._lock = threading.Lock()
        # WS state
        self._ws_thread: Optional[threading.Thread] = None
        self._ws_running = False
        self._subscribed_tokens: set = set()
        self._best_asks: Dict[str, float] = {}

    def place_gtc_order(self, token_id: str, side: str, price: float, shares: float) -> str:
        if not token_id:
            raise ValueError("token_id required")
        if shares <= 0 or price <= 0:
            raise ValueError(f"Invalid order: shares={shares} price={price}")

        oid = f"paper-{self._next_id}"
        self._next_id += 1

        # Sweep all ask levels up to our price for immediate fills
        filled = self._try_immediate_fill(oid, token_id, side, price, shares)

        remaining = shares - filled
        if remaining > 0:
            with self._lock:
                self._orders[oid] = {
                    "token_id": token_id, "side": side,
                    "price": price, "shares": remaining,
                }
            log.info(f"[PAPER] Resting order {oid}: {side} {remaining:.0f} @ ${price:.4f}")
            self._ensure_ws_subscription(token_id)

        return oid

    def _try_immediate_fill(self, oid: str, token_id: str, side: str, price: float, shares: float) -> float:
        """Sweep the orderbook for immediate fills. Returns shares filled."""
        book = self._fetch_book(token_id)
        asks = book.get("asks", [])
        if side != "BUY" or not asks:
            return 0

        total_filled = 0
        remaining = shares
        for ask in asks:
            ask_price = float(ask.get("price", 999))
            ask_size = float(ask.get("size", 0))
            if ask_price > price:
                break
            fill_shares = min(remaining, ask_size)
            with self._lock:
                self._fills.append((oid, ask_price, fill_shares, time.time()))
            total_filled += fill_shares
            remaining -= fill_shares
            log.info(f"[PAPER] Immediate fill {oid}: {fill_shares:.0f} @ ${ask_price:.4f}")
            if remaining <= 0:
                break
        return total_filled

    def tick(self):
        """Check resting orders against WS best_ask. Also REST fallback."""
        with self._lock:
            filled_oids = []
            for oid, order in list(self._orders.items()):
                token_id = order["token_id"]
                best_ask = self._best_asks.get(token_id)
                if best_ask is None:
                    book = self._fetch_book(token_id)
                    asks = book.get("asks", [])
                    if asks:
                        best_ask = float(asks[0].get("price", 999))

                if best_ask is not None and order["side"] == "BUY" and order["price"] >= best_ask:
                    self._fills.append((oid, best_ask, order["shares"], time.time()))
                    log.info(f"[PAPER] Fill {oid}: {order['shares']:.0f} @ ${best_ask:.4f} (tick)")
                    filled_oids.append(oid)
            for oid in filled_oids:
                del self._orders[oid]

    def cancel_order(self, order_id: str):
        with self._lock:
            self._orders.pop(order_id, None)

    def cancel_all(self):
        with self._lock:
            self._orders.clear()

    def get_fills(self) -> List[Tuple[str, float, float, float]]:
        with self._lock:
            return list(self._fills)

    def get_open_orders(self) -> List[str]:
        with self._lock:
            return list(self._orders.keys())

    def reset(self):
        self.cancel_all()
        with self._lock:
            self._fills.clear()

    def stop(self):
        self._ws_running = False
        if self._ws_thread:
            self._ws_thread.join(timeout=3)

    # ── WebSocket ────────────────────────────────────────────────────────

    def _ensure_ws_subscription(self, token_id: str):
        if token_id in self._subscribed_tokens:
            return
        self._subscribed_tokens.add(token_id)
        if not self._ws_running:
            self._ws_running = True
            self._ws_thread = threading.Thread(target=self._ws_loop, daemon=True)
            self._ws_thread.start()

    def _ws_loop(self):
        import websockets.sync.client as ws_sync
        while self._ws_running:
            try:
                ws = ws_sync.connect(PM_WS_URL)
                log.info("[PAPER] Polymarket WS connected")
                if self._subscribed_tokens:
                    ws.send(json.dumps({
                        "assets_ids": list(self._subscribed_tokens),
                        "type": "market",
                    }))
                    log.info(f"[PAPER] Subscribed to {len(self._subscribed_tokens)} tokens")
                while self._ws_running:
                    try:
                        msg = ws.recv(timeout=3)
                    except (TimeoutError, Exception):
                        continue
                    if msg in ("PONG", "[]"):
                        continue
                    try:
                        data = json.loads(msg)
                    except json.JSONDecodeError:
                        continue
                    if isinstance(data, list):
                        for item in data:
                            self._process_ws_message(item)
                    elif isinstance(data, dict):
                        self._process_ws_message(data)
                ws.close()
            except Exception as e:
                if self._ws_running:
                    log.warning(f"[PAPER] Polymarket WS error: {e}, reconnecting in 2s")
                    time.sleep(2)

    def _process_ws_message(self, msg: dict):
        # Full book snapshot
        if "bids" in msg and "asks" in msg:
            asset_id = msg.get("asset_id", "")
            asks = msg.get("asks", [])
            if asks and asset_id:
                best_ask = float(asks[0].get("price", 999))
                self._best_asks[asset_id] = best_ask
                self._check_resting_fills(asset_id, best_ask)
            return
        # Price change events
        for change in msg.get("price_changes", []):
            asset_id = change.get("asset_id", "")
            best_ask_str = change.get("best_ask")
            if best_ask_str and asset_id:
                best_ask = float(best_ask_str)
                self._best_asks[asset_id] = best_ask
                self._check_resting_fills(asset_id, best_ask)

    def _check_resting_fills(self, token_id: str, best_ask: float):
        with self._lock:
            filled_oids = []
            for oid, order in list(self._orders.items()):
                if order["token_id"] != token_id or order["side"] != "BUY":
                    continue
                if order["price"] >= best_ask:
                    self._fills.append((oid, best_ask, order["shares"], time.time()))
                    log.info(f"[PAPER] WS Fill {oid}: {order['shares']:.0f} @ ${best_ask:.4f}")
                    filled_oids.append(oid)
            for oid in filled_oids:
                del self._orders[oid]

    def _fetch_book(self, token_id: str) -> dict:
        try:
            resp = requests.get(f"{CLOB_REST}/book", params={"token_id": token_id}, timeout=5)
            return resp.json()
        except Exception as e:
            log.warning(f"Book fetch error: {e}")
            return {"asks": [], "bids": []}


# ---------------------------------------------------------------------------
# LiveExecutor / MicroLiveExecutor
# ---------------------------------------------------------------------------


class LiveExecutor:
    def __init__(self, private_key: str, chain_id: int = 137):
        if not HAS_CLOB_CLIENT:
            raise ImportError("py-clob-client required for live mode: pip install py-clob-client")
        self._client = ClobClient(
            CLOB_REST,
            key=private_key,
            chain_id=chain_id,
        )
        self._client.set_api_creds(self._client.create_or_derive_api_creds())
        self._orders: Dict[str, dict] = {}
        self._fills: List[Tuple[str, float, float, float]] = []

    def place_gtc_order(self, token_id: str, side: str, price: float, shares: float) -> str:
        if not token_id:
            raise ValueError("token_id required")
        if shares <= 0 or price <= 0:
            raise ValueError(f"Invalid order: shares={shares} price={price}")
        clob_side = BUY
        order_args = OrderArgs(token_id=token_id, price=price, size=shares, side=clob_side)
        signed = self._client.create_order(order_args)
        resp = self._client.post_order(signed, OrderType.GTC)
        oid = resp.get("orderID", "")
        self._orders[oid] = {"token_id": token_id, "price": price, "shares": shares}
        log.info(f"[LIVE] Posted GTC {oid}: BUY {shares:.0f} @ ${price:.4f}")
        return oid

    def cancel_order(self, order_id: str):
        try:
            self._client.cancel(order_id=order_id)
            self._orders.pop(order_id, None)
            log.info(f"[LIVE] Cancelled {order_id}")
        except Exception as e:
            log.warning(f"Cancel error for {order_id}: {e}")

    def cancel_all(self):
        try:
            self._client.cancel_all()
            self._orders.clear()
            log.info("[LIVE] Cancelled all orders")
        except Exception as e:
            log.warning(f"Cancel all error: {e}")

    def get_fills(self) -> List[Tuple[str, float, float, float]]:
        for oid in list(self._orders.keys()):
            try:
                order = self._client.get_order(oid)
                matched = float(order.get("size_matched", 0))
                if matched > 0:
                    price = float(order.get("price", 0))
                    existing = sum(f[2] for f in self._fills if f[0] == oid)
                    new_fill = matched - existing
                    if new_fill > 0:
                        self._fills.append((oid, price, new_fill, time.time()))
            except Exception as e:
                log.warning(f"Fill check error for {oid}: {e}")
        return list(self._fills)

    def get_open_orders(self) -> List[str]:
        return list(self._orders.keys())

    def reset(self):
        self.cancel_all()
        self._fills.clear()


class MicroLiveExecutor(LiveExecutor):
    def __init__(self, private_key: str, micro_size: float = 1.0, chain_id: int = 137):
        super().__init__(private_key, chain_id)
        self.micro_size = micro_size

    def place_gtc_order(self, token_id: str, side: str, price: float, shares: float) -> str:
        capped = min(shares, self.micro_size)
        log.info(f"[MICRO] Capping {shares:.0f} → {capped:.1f} shares")
        return super().place_gtc_order(token_id, side, price, capped)


# ---------------------------------------------------------------------------
# OrderManager
# ---------------------------------------------------------------------------


class OrderManager:
    def __init__(self, executor, position: Position, config: dict):
        self.executor = executor
        self.position = position
        self.config = config
        self.side_switches = 0
        self.current_direction: Optional[str] = None
        self._favored_orders: List[str] = []
        self._insurance_orders: List[str] = []

    def place_favored(self, token_id: str, price: float, shares: float) -> bool:
        if price > self.config["favored_max_price"]:
            log.warning(f"Rejected: price ${price:.4f} > max ${self.config['favored_max_price']}")
            return False
        projected = self.position.total_deployed + (shares * price)
        if projected > self.config["max_deploy_per_window"]:
            log.warning(f"Rejected: projected ${projected:.0f} > budget ${self.config['max_deploy_per_window']}")
            return False
        oid = self.executor.place_gtc_order(token_id, "BUY", price, shares)
        self._favored_orders.append(oid)
        return True

    def place_insurance(self, token_id: str, price: float, shares: float) -> bool:
        if price > self.config["insurance_max_price"]:
            log.warning(f"Rejected insurance: price ${price:.4f} > max ${self.config['insurance_max_price']}")
            return False
        oid = self.executor.place_gtc_order(token_id, "BUY", price, shares)
        self._insurance_orders.append(oid)
        return True

    def post_favored_ladder(self, token_id: str, direction: str):
        self.current_direction = direction
        shares = self.config["favored_shares"]
        if self.config["mode"] == "micro-live":
            shares = self.config["micro_live_size"]
        for price in self.config["favored_prices"]:
            self.place_favored(token_id, price, shares)

    def post_insurance(self, token_id: str):
        shares = self.config["insurance_shares"]
        if self.config["mode"] == "micro-live":
            shares = self.config["micro_live_size"]
        for price in self.config["insurance_prices"]:
            self.place_insurance(token_id, price, shares)

    def cancel_all(self):
        self.executor.cancel_all()
        self._favored_orders.clear()
        self._insurance_orders.clear()

    def update_fills(self):
        for oid, price, shares, ts in self.executor.get_fills():
            side = self._side_for_order(oid)
            usdc = shares * price
            already = any(f.order_id == oid and f.ts == ts for f in self.position.fills)
            if not already:
                self.position.add_fill(side, shares, price, usdc, ts=ts,
                                       order_type=self._type_for_order(oid), order_id=oid)
                log.info(f"Fill: {side} {shares:.0f} @ ${price:.4f} = ${usdc:.2f}")

    def _side_for_order(self, oid: str) -> str:
        if self.current_direction is None:
            log.warning(f"_side_for_order called with no direction for {oid}, defaulting to Up")
        if oid in self._favored_orders:
            return self.current_direction or "Up"
        else:
            return "Down" if self.current_direction == "Up" else "Up"

    def _type_for_order(self, oid: str) -> str:
        return "favored" if oid in self._favored_orders else "insurance"


# ---------------------------------------------------------------------------
# Task 8: SettlementHandler + WindowRecorder
# ---------------------------------------------------------------------------

class SettlementHandler:
    def resolve(self, slug: str, condition_id: str, market_id: Optional[int] = None,
                token_up: str = "", token_down: str = "",
                retries: int = 5, delay: float = 10) -> Optional[str]:
        for attempt in range(retries):
            try:
                # Approach A: direct market lookup by numeric ID
                if market_id:
                    resp = requests.get(f"{GAMMA_API}/markets/{market_id}", timeout=10)
                    if resp.status_code == 200:
                        mkt = resp.json()
                        prices = mkt.get("outcomePrices", "")
                        outcomes = mkt.get("outcomes", "")
                        # Parse — these might be JSON strings or already lists
                        if isinstance(prices, str):
                            prices = json.loads(prices)
                        if isinstance(outcomes, str):
                            outcomes = json.loads(outcomes)
                        if len(prices) == 2 and len(outcomes) == 2:
                            p0, p1 = float(prices[0]), float(prices[1])
                            if abs(p0 - p1) > 0.5:  # clearly resolved
                                resolved = outcomes[0] if p0 > p1 else outcomes[1]
                                return resolved
            except Exception as e:
                log.warning(f"Resolution attempt {attempt+1} failed: {e}")
            if attempt < retries - 1:
                time.sleep(delay)

        # Approach B: fallback — check orderbook prices
        if token_up and token_down:
            try:
                for token_id, outcome in [(token_up, "Up"), (token_down, "Down")]:
                    book = requests.get(
                        f"{CLOB_REST}/book",
                        params={"token_id": token_id},
                        timeout=5,
                    ).json()
                    bids = book.get("bids", [])
                    if bids and float(bids[0].get("price", 0)) > 0.90:
                        return outcome
            except Exception as e:
                log.warning(f"Orderbook fallback failed: {e}")

        log.error(f"Could not resolve {slug} after {retries} attempts")
        return None


class WindowRecorder:
    def __init__(self, replay_dir: str = "window_replay/"):
        self.replay_dir = Path(replay_dir)
        self.replay_dir.mkdir(parents=True, exist_ok=True)

    def write(self, market: Market, position: Position, winner: Optional[str],
              signal: dict, spy_data: Optional[dict] = None):
        open_dt = datetime.fromtimestamp(market.window_open, tz=timezone.utc)
        filename = f"{open_dt.strftime('%Y-%m-%d_%H-%M')}_btc-15m.json"

        up_avg = position.up_cost / position.up_shares if position.up_shares > 0 else 0
        dn_avg = position.down_cost / position.down_shares if position.down_shares > 0 else 0
        pnl = position.pnl_if(winner) if winner else 0
        deployed = position.total_deployed
        roi = pnl / deployed * 100 if deployed > 0 else 0

        record = {
            "window": {
                "slug": market.slug,
                "open_time": open_dt.isoformat(),
                "close_time": datetime.fromtimestamp(market.window_close, tz=timezone.utc).isoformat(),
                "winner": winner,
            },
            "signal": signal,
            "our_trades": [
                {"time": datetime.fromtimestamp(f.ts, tz=timezone.utc).isoformat(),
                 "side": f.side, "price": f.price, "shares": f.shares,
                 "usdc": f.usdc, "type": f.order_type}
                for f in position.fills
            ],
            "our_position": {
                "up_shares": position.up_shares,
                "up_cost": round(position.up_cost, 2),
                "up_avg_price": round(up_avg, 4),
                "down_shares": position.down_shares,
                "down_cost": round(position.down_cost, 2),
                "down_avg_price": round(dn_avg, 4),
                "total_deployed": round(deployed, 2),
                "pnl": round(pnl, 2),
                "roi_pct": round(roi, 1),
            },
            "spy": spy_data,
        }

        if spy_data and winner:
            # Calculate spy PnL from their position
            spy_up_sh = spy_data.get("up_shares", 0)
            spy_up_c = spy_data.get("up_cost", 0)
            spy_dn_sh = spy_data.get("down_shares", 0)
            spy_dn_c = spy_data.get("down_cost", 0)
            spy_total = spy_up_c + spy_dn_c
            if winner == "Up":
                spy_pnl = spy_up_sh - spy_up_c - spy_dn_c
            else:
                spy_pnl = spy_dn_sh - spy_dn_c - spy_up_c
            spy_roi = spy_pnl / spy_total * 100 if spy_total > 0 else 0

            spy_w_shares = spy_up_sh if winner == "Up" else spy_dn_sh
            spy_w_cost = spy_up_c if winner == "Up" else spy_dn_c
            spy_w_avg = spy_w_cost / spy_w_shares if spy_w_shares > 0 else 0

            record["comparison"] = {
                "our_pnl": round(pnl, 2),
                "spy_pnl": round(spy_pnl, 2),
                "spy_roi_pct": round(spy_roi, 1),
                "same_direction": signal.get("direction") == spy_data.get("direction"),
                "our_w_avg": round(up_avg if winner == "Up" else dn_avg, 4),
                "spy_w_avg": round(spy_w_avg, 4),
            }

        filepath = self.replay_dir / filename
        filepath.write_text(json.dumps(record, indent=2, default=str))
        log.info(f"Replay saved: {filepath}")


# ---------------------------------------------------------------------------
# Task 9: SpyThread
# ---------------------------------------------------------------------------

HEISENBERG_API = "https://narrative.agent.heisenberg.so/api/v2/semantic/retrieve/parameterized"


class SpyThread:
    def __init__(self, wallet: str, api_key: str,
                 window_open: int, window_close: int, slug: str,
                 poll_interval: float = 5.0):
        self.wallet = wallet
        self.api_key = api_key
        self.window_open = window_open
        self.window_close = window_close
        self.slug = slug
        self.poll_interval = poll_interval
        self._data: Dict = {}
        self._lock = threading.Lock()
        self._running = False
        self._thread: Optional[threading.Thread] = None

    def start(self):
        self._running = True
        self._thread = threading.Thread(target=self._run, daemon=True)
        self._thread.start()

    def stop(self):
        self._running = False
        if self._thread:
            self._thread.join(timeout=3)

    def get_data(self) -> dict:
        with self._lock:
            return dict(self._data)

    def _run(self):
        while self._running:
            self._poll_once()
            time.sleep(self.poll_interval)
        self._poll_once()

    def _poll_once(self):
        try:
            resp = requests.post(
                HEISENBERG_API,
                headers={
                    "Authorization": f"Bearer {self.api_key}",
                    "Content-Type": "application/json",
                },
                json={
                    "agent_id": 556,
                    "params": {
                        "proxy_wallet": self.wallet,
                        "condition_id": "ALL",
                        "start_time": str(self.window_open),
                        "end_time": str(self.window_close),
                    },
                    "pagination": {"limit": 200, "offset": 0},
                    "formatter_config": {"format_type": "raw"},
                },
                timeout=15,
            )
            if resp.status_code != 200:
                return

            result = resp.json()
            trades = []
            if isinstance(result, dict):
                if "data" in result and isinstance(result["data"], dict):
                    trades = result["data"].get("results", [])
                elif "data" in result and isinstance(result["data"], list):
                    trades = result["data"]
                elif "results" in result:
                    trades = result["results"]

            slug_lower = self.slug.lower()
            window_trades = [t for t in trades
                           if slug_lower in t.get("slug", "").lower()]

            up_shares = sum(float(t.get("size", 0)) for t in window_trades if t.get("outcome") == "Up")
            up_cost = sum(float(t.get("size", 0)) * float(t.get("price", 0))
                        for t in window_trades if t.get("outcome") == "Up")
            dn_shares = sum(float(t.get("size", 0)) for t in window_trades if t.get("outcome") == "Down")
            dn_cost = sum(float(t.get("size", 0)) * float(t.get("price", 0))
                        for t in window_trades if t.get("outcome") == "Down")

            direction = "Down" if dn_cost > up_cost else "Up" if up_cost > dn_cost else None
            total = up_cost + dn_cost
            w_avg = (dn_cost / dn_shares if direction == "Down" and dn_shares > 0
                    else up_cost / up_shares if direction == "Up" and up_shares > 0
                    else 0)

            with self._lock:
                self._data = {
                    "wallet": self.wallet,
                    "trades_count": len(window_trades),
                    "direction": direction,
                    "up_shares": up_shares, "up_cost": round(up_cost, 2),
                    "down_shares": dn_shares, "down_cost": round(dn_cost, 2),
                    "total_deployed": round(total, 2),
                    "w_avg": round(w_avg, 4),
                    "position": {
                        "up_shares": up_shares, "up_cost": round(up_cost, 2),
                        "down_shares": dn_shares, "down_cost": round(dn_cost, 2),
                    },
                    "raw_trades": window_trades[:20],
                }
        except Exception as e:
            log.warning(f"Spy poll error: {e}")


# ---------------------------------------------------------------------------
# Task 10: Main Loop + CLI
# ---------------------------------------------------------------------------


def run_one_window(config: dict, market: Market, direction: str,
                   signal_data: dict, winner: Optional[str] = None,
                   spy: Optional["SpyThread"] = None) -> dict:
    """Execute one full window cycle. Returns result dict."""
    # Create executor based on mode
    if config["mode"] == "paper":
        executor = PaperExecutor()
    elif config["mode"] == "micro-live":
        pk = os.environ.get("POLYMARKET_PRIVATE_KEY", "")
        executor = MicroLiveExecutor(private_key=pk, micro_size=config["micro_live_size"])
    else:
        pk = os.environ.get("POLYMARKET_PRIVATE_KEY", "")
        executor = LiveExecutor(private_key=pk)

    position = Position()
    om = OrderManager(executor=executor, position=position, config=config)
    recorder = WindowRecorder(replay_dir=config["replay_dir"])

    # Determine tokens based on direction
    if direction == "Up":
        favored_token = market.token_up
        insurance_token = market.token_down
    else:
        favored_token = market.token_down
        insurance_token = market.token_up

    # Phase 1: Post favored ladder
    log.info(f"Posting favored ladder: {direction} on {market.slug}")
    om.post_favored_ladder(favored_token, direction)

    # Phase 2: Wait through window, ticking executor and managing phases
    wm = WindowManager(config["window_duration"])
    wm.set_window(market.window_open)
    insurance_posted = False
    stop_reached = False

    while True:
        pct = wm.pct_through()

        # Tick paper executor for fill simulation
        if isinstance(executor, PaperExecutor):
            executor.tick()
        om.update_fills()

        # Post insurance at configured %
        if not insurance_posted and pct >= config["insurance_start_pct"]:
            log.info(f"Posting insurance at {pct:.0f}%")
            om.post_insurance(insurance_token)
            insurance_posted = True

        # Stop trading at configured %
        if not stop_reached and pct >= config["stop_trading_pct"]:
            log.info(f"Stop trading at {pct:.0f}%, cancelling unfilled")
            om.cancel_all()
            stop_reached = True

        # Window closed?
        if pct >= 100:
            break

        time.sleep(2)

    # Final fill update
    if isinstance(executor, PaperExecutor):
        executor.tick()
    om.update_fills()

    # Resolve winner if not provided (test mode provides it)
    if winner is None:
        sh = SettlementHandler()
        winner = sh.resolve(slug=market.slug, condition_id=market.condition_id,
                            market_id=market.market_id,
                            token_up=market.token_up, token_down=market.token_down)

    # Compute PnL
    pnl = position.pnl_if(winner) if winner else 0
    deployed = position.total_deployed
    roi = pnl / deployed * 100 if deployed > 0 else 0

    log.info(f"Window complete: winner={winner} PnL=${pnl:+,.2f} ({roi:+.1f}%) deployed=${deployed:,.2f}")

    # Collect spy data before writing replay
    spy_data = None
    if spy:
        spy.stop()
        spy_data = spy.get_data()
        # Calculate spy PnL for logging
        spy_up_sh = spy_data.get("up_shares", 0)
        spy_dn_sh = spy_data.get("down_shares", 0)
        spy_total = spy_data.get("total_deployed", 0)
        if winner == "Up":
            spy_pnl_val = spy_up_sh - spy_total
        elif winner == "Down":
            spy_pnl_val = spy_dn_sh - spy_total
        else:
            spy_pnl_val = 0
        log.info(f"Spy: {spy_data.get('direction')} | deployed=${spy_total:,.0f} | "
                 f"PnL=${spy_pnl_val:+,.0f} | Up:{spy_up_sh:.0f} Down:{spy_dn_sh:.0f}")

    # Record replay
    recorder.write(market=market, position=position, winner=winner,
                   signal=signal_data, spy_data=spy_data)

    return {"pnl": pnl, "deployed": deployed, "roi": roi, "winner": winner}


def main():
    parser = argparse.ArgumentParser(description="BTC 15m Up/Down Trading Bot")
    parser.add_argument("--mode", choices=["paper", "micro-live", "live"], default="paper")
    parser.add_argument("--spy", type=str, default=None, help="Wallet address to spy on")
    args = parser.parse_args()

    config = dict(CONFIG)
    config["mode"] = args.mode
    if args.spy:
        config["spy_enabled"] = True
        config["spy_wallet"] = args.spy

    # Setup logging
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s [%(levelname)s] %(message)s",
        datefmt="%H:%M:%S",
        handlers=[
            logging.StreamHandler(),
            logging.FileHandler(config["log_file"]),
        ],
    )

    log.info(f"Starting BTC 15m bot | mode={config['mode']} | spy={'ON' if config['spy_enabled'] else 'OFF'}")

    # Start Binance WS
    signal = SignalEngine(min_delta=config["min_btc_delta_usd"])
    signal.start_ws()
    time.sleep(2)  # wait for first price

    discovery = MarketDiscovery(asset=config["asset"])
    wm = WindowManager(config["window_duration"])

    daily_pnl = 0.0
    consecutive_losses = 0

    try:
        while True:
            # Check daily loss limit
            if daily_pnl <= -config["max_daily_loss"]:
                log.error(f"Daily loss limit reached: ${daily_pnl:+,.2f}. Halting.")
                break
            if consecutive_losses >= config["max_consecutive_losses"]:
                log.error(f"Max consecutive losses ({consecutive_losses}). Halting.")
                break

            # Wait for next window
            window_open = wm.wait_for_window()

            # Discover market
            market = discovery.find_market(window_open)
            if market is None:
                log.warning(f"No BTC 15m market found for window {window_open}. Skipping.")
                continue

            log.info(f"Window: {market.slug}")

            # Wait for signal
            direction = signal.wait_for_signal(config["signal_delay_sec"])
            if direction is None:
                log.info("No clear signal. Skipping window.")
                continue

            signal_data = {
                "btc_open_price": signal.open_price,
                "btc_at_signal": signal.current_price,
                "delta": signal.delta(),
                "direction": direction,
            }

            # Start spy if enabled
            spy = None
            if config["spy_enabled"]:
                spy = SpyThread(
                    wallet=config["spy_wallet"],
                    api_key=config.get("heisenberg_api_key", ""),
                    window_open=window_open,
                    window_close=window_open + config["window_duration"],
                    slug=market.slug,
                    poll_interval=config["spy_poll_interval_sec"],
                )
                spy.start()

            # Run the window (spy is stopped and data collected inside)
            result = run_one_window(config=config, market=market,
                                    direction=direction, signal_data=signal_data,
                                    spy=spy)

            # Track daily PnL
            daily_pnl += result["pnl"]
            if result["pnl"] >= 0:
                consecutive_losses = 0
            else:
                consecutive_losses += 1

            log.info(f"Daily PnL: ${daily_pnl:+,.2f} | Streak: {consecutive_losses} losses")

    except KeyboardInterrupt:
        log.info("Shutting down...")
    finally:
        signal.stop_ws()
        log.info(f"Session complete. Daily PnL: ${daily_pnl:+,.2f}")


if __name__ == "__main__":
    main()
