"""
Improved PaperExecutor v2 — Uses Polymarket WebSocket for realistic fill simulation.

Instead of polling orderbook snapshots, this executor:
1. At placement time: fetches the book and sweeps asks up to our price (immediate fill)
2. For resting orders: subscribes to the Polymarket market WS
3. When price_changes show the best_ask dropping to our resting price → fill

This can be dropped into btc_15m_bot.py to replace PaperExecutor.
"""

from __future__ import annotations

import json
import logging
import threading
import time
from typing import Dict, List, Optional, Tuple

import requests
import websockets.sync.client as ws_sync

log = logging.getLogger("btc15m")

CLOB_REST = "https://clob.polymarket.com"
PM_WS_URL = "wss://ws-subscriptions-clob.polymarket.com/ws/market"


class PaperExecutorV2:
    """Paper executor with real-time WebSocket fill simulation."""

    def __init__(self):
        self._orders: Dict[str, dict] = {}  # oid -> {token_id, side, price, shares}
        self._fills: List[Tuple[str, float, float, float]] = []  # (oid, price, shares, ts)
        self._next_id = 0
        self._lock = threading.Lock()

        # WS state
        self._ws_thread: Optional[threading.Thread] = None
        self._ws_running = False
        self._subscribed_tokens: set = set()
        self._best_asks: Dict[str, float] = {}  # token_id -> best ask price

    # ── Order Management ─────────────────────────────────────────────────

    def place_gtc_order(self, token_id: str, side: str, price: float, shares: float) -> str:
        if not token_id:
            raise ValueError("token_id required")
        if shares <= 0 or price <= 0:
            raise ValueError(f"Invalid order: shares={shares} price={price}")

        oid = f"paper-{self._next_id}"
        self._next_id += 1

        # Try immediate fill against current orderbook
        filled = self._try_immediate_fill(oid, token_id, side, price, shares)

        if not filled:
            # Resting order — will be checked by WS stream
            with self._lock:
                self._orders[oid] = {
                    "token_id": token_id, "side": side,
                    "price": price, "shares": shares,
                }
            log.info(f"[PAPER] Resting order {oid}: {side} {shares:.0f} @ ${price:.4f}")

            # Subscribe to this token's WS if not already
            self._ensure_ws_subscription(token_id)
        elif filled < shares:
            # Partial fill — rest stays on book
            remaining = shares - filled
            with self._lock:
                self._orders[oid] = {
                    "token_id": token_id, "side": side,
                    "price": price, "shares": remaining,
                }
            self._ensure_ws_subscription(token_id)

        return oid

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
        """Stop the WS thread."""
        self._ws_running = False
        if self._ws_thread:
            self._ws_thread.join(timeout=3)

    # ── Tick (kept for backwards compat, but WS does the real work) ──────

    def tick(self):
        """Check resting orders against current best_ask from WS.
        Also serves as a fallback if WS is not streaming."""
        with self._lock:
            filled_oids = []
            for oid, order in list(self._orders.items()):
                token_id = order["token_id"]
                best_ask = self._best_asks.get(token_id)

                if best_ask is None:
                    # No WS data yet — poll the REST book as fallback
                    book = self._fetch_book(token_id)
                    asks = book.get("asks", [])
                    if asks:
                        best_ask = float(asks[0].get("price", 999))

                if best_ask is not None and order["side"] == "BUY" and order["price"] >= best_ask:
                    fill_price = best_ask
                    fill_shares = order["shares"]  # fill full amount at best ask
                    self._fills.append((oid, fill_price, fill_shares, time.time()))
                    log.info(f"[PAPER] Fill {oid}: {fill_shares:.0f} @ ${fill_price:.4f} (tick)")
                    filled_oids.append(oid)

            for oid in filled_oids:
                del self._orders[oid]

    # ── Immediate Fill (at order placement) ──────────────────────────────

    def _try_immediate_fill(self, oid: str, token_id: str, side: str, price: float, shares: float) -> float:
        """Try to fill immediately by sweeping the orderbook. Returns shares filled."""
        book = self._fetch_book(token_id)
        asks = book.get("asks", [])

        if side != "BUY" or not asks:
            return 0

        total_filled = 0
        remaining = shares

        # Sweep asks up to our price (not just top level)
        for ask in asks:
            ask_price = float(ask.get("price", 999))
            ask_size = float(ask.get("size", 0))

            if ask_price > price:
                break  # asks are sorted, rest will be higher

            fill_shares = min(remaining, ask_size)
            with self._lock:
                self._fills.append((oid, ask_price, fill_shares, time.time()))
            total_filled += fill_shares
            remaining -= fill_shares
            log.info(f"[PAPER] Immediate fill {oid}: {fill_shares:.0f} @ ${ask_price:.4f}")

            if remaining <= 0:
                break

        return total_filled

    # ── WebSocket Stream ─────────────────────────────────────────────────

    def _ensure_ws_subscription(self, token_id: str):
        """Start WS thread if needed and subscribe to token."""
        if token_id in self._subscribed_tokens:
            return

        self._subscribed_tokens.add(token_id)

        if not self._ws_running:
            self._ws_running = True
            self._ws_thread = threading.Thread(target=self._ws_loop, daemon=True)
            self._ws_thread.start()

    def _ws_loop(self):
        """WebSocket loop: subscribe to tokens, process price changes."""
        while self._ws_running:
            try:
                ws = ws_sync.connect(PM_WS_URL)
                log.info("[PAPER] Polymarket WS connected")

                # Subscribe to all tokens we care about
                if self._subscribed_tokens:
                    ws.send(json.dumps({
                        "assets_ids": list(self._subscribed_tokens),
                        "type": "market",
                    }))
                    log.info(f"[PAPER] Subscribed to {len(self._subscribed_tokens)} tokens")

                # Process messages
                while self._ws_running:
                    try:
                        msg = ws.recv(timeout=3)
                    except TimeoutError:
                        continue
                    except Exception:
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
        """Process a WS message: update best asks, check for fills."""

        # Full book snapshot — update best asks
        if "bids" in msg and "asks" in msg:
            asset_id = msg.get("asset_id", "")
            asks = msg.get("asks", [])
            if asks and asset_id:
                best_ask = float(asks[0].get("price", 999))
                self._best_asks[asset_id] = best_ask
                self._check_resting_fills(asset_id, best_ask)
            return

        # Price change events
        price_changes = msg.get("price_changes", [])
        for change in price_changes:
            asset_id = change.get("asset_id", "")
            best_ask_str = change.get("best_ask")
            if best_ask_str and asset_id:
                best_ask = float(best_ask_str)
                self._best_asks[asset_id] = best_ask
                self._check_resting_fills(asset_id, best_ask)

    def _check_resting_fills(self, token_id: str, best_ask: float):
        """Check if any resting orders can fill at the current best ask."""
        with self._lock:
            filled_oids = []
            for oid, order in list(self._orders.items()):
                if order["token_id"] != token_id:
                    continue
                if order["side"] != "BUY":
                    continue
                if order["price"] >= best_ask:
                    fill_price = best_ask
                    fill_shares = order["shares"]
                    self._fills.append((oid, fill_price, fill_shares, time.time()))
                    log.info(f"[PAPER] WS Fill {oid}: {fill_shares:.0f} @ ${fill_price:.4f}")
                    filled_oids.append(oid)

            for oid in filled_oids:
                del self._orders[oid]

    # ── REST Fallback ────────────────────────────────────────────────────

    def _fetch_book(self, token_id: str) -> dict:
        try:
            resp = requests.get(
                f"{CLOB_REST}/book",
                params={"token_id": token_id},
                timeout=5,
            )
            return resp.json()
        except Exception as e:
            log.warning(f"[PAPER] Book fetch error: {e}")
            return {"asks": [], "bids": []}
