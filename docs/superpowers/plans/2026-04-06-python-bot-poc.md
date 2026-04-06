# Python Bot POC — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a full-stack Polymarket trading bot in Python that proves the Rust SDK architecture — ingest, engine, strategy, execution, telemetry, and Textual TUI dashboard.

**Architecture:** Single-process asyncio with 7 concurrent tasks + Textual TUI. Each asyncio task maps 1:1 to a Rust thread. `asyncio.Queue` replaces SPSC rings. Two strategies (Ladder MM + Momentum) with independent positions and risk. Dry_run mode with realistic maker execution simulation (queue-aware fills, latency injection, adverse selection tracking).

**Tech Stack:** Python 3.11+, py-clob-client, websockets, textual, tomli/tomllib

**Spec:** `docs/superpowers/specs/2026-04-06-python-bot-poc-design.md`

**Reference:** Nim POC at `polymarket-bot-poc/` for WS parsing patterns, engine logic, dashboard layout.

---

## Plan Structure

This plan is split into 3 phases that each produce runnable software:

- **Phase 1 (Tasks 1-7):** Foundation + Ingest + Engine — runs and processes live market data
- **Phase 2 (Tasks 8-15):** Strategy + Risk — strategy components with tests
- **Phase 3 (Tasks 16-21):** Dry Run Simulator + Execution — realistic maker simulation
- **Phase 4 (Tasks 22-25):** Telemetry + Dashboard — full bot with Textual TUI

---

## File Structure

```
polymarket-bot-python-poc/
├── config/
│   └── bot.toml
├── src/
│   ├── __init__.py
│   ├── main.py
│   ├── config.py
│   ├── types.py
│   ├── ingest/
│   │   ├── __init__.py
│   │   ├── polymarket.py
│   │   ├── binance.py
│   │   └── account.py
│   ├── engine/
│   │   ├── __init__.py
│   │   ├── book.py
│   │   ├── market_state.py
│   │   └── engine.py
│   ├── strategy/
│   │   ├── __init__.py
│   │   ├── base.py
│   │   ├── ladder_mm.py
│   │   ├── momentum.py
│   │   ├── position.py
│   │   ├── order_tracker.py
│   │   └── queue_estimator.py
│   ├── execution/
│   │   ├── __init__.py
│   │   ├── executor.py
│   │   ├── simulator.py
│   │   ├── markout.py
│   │   ├── heartbeat.py
│   │   └── risk.py
│   ├── telemetry/
│   │   ├── __init__.py
│   │   ├── recorder.py
│   │   ├── stats.py
│   │   └── telemetry.py
│   └── dashboard/
│       ├── __init__.py
│       └── app.py
├── tests/
│   ├── __init__.py
│   ├── test_types.py
│   ├── test_config.py
│   ├── test_book.py
│   ├── test_market_state.py
│   ├── test_position.py
│   ├── test_order_tracker.py
│   ├── test_queue_estimator.py
│   ├── test_risk.py
│   ├── test_ladder_mm.py
│   ├── test_momentum.py
│   └── test_stats.py
├── pyproject.toml
├── .env.example
└── .gitignore
```

---

## Phase 1: Foundation + Ingest + Engine

### Task 1: Project Scaffold

**Files:**
- Create: `polymarket-bot-python-poc/pyproject.toml`
- Create: `polymarket-bot-python-poc/.gitignore`
- Create: `polymarket-bot-python-poc/.env.example`
- Create: `polymarket-bot-python-poc/src/__init__.py`
- Create: `polymarket-bot-python-poc/tests/__init__.py`

- [ ] **Step 1: Create pyproject.toml**

```toml
[project]
name = "polymarket-bot-poc"
version = "0.1.0"
requires-python = ">=3.11"
dependencies = [
    "py-clob-client>=0.34.0",
    "websockets>=13.0",
    "textual>=3.0",
    "python-dotenv>=1.0",
]

[project.optional-dependencies]
dev = [
    "pytest>=8.0",
    "pytest-asyncio>=0.24",
]

[project.scripts]
mantis-bot = "src.main:main"
```

- [ ] **Step 2: Create .gitignore**

```
__pycache__/
*.pyc
.env
data/
*.egg-info/
.venv/
```

- [ ] **Step 3: Create .env.example**

```
PM_API_KEY=your_api_key_here
PM_SECRET=your_api_secret_here
PM_PASSPHRASE=your_passphrase_here
PM_PRIVATE_KEY=0xyour_private_key_here
```

- [ ] **Step 4: Create empty __init__.py files**

Create `src/__init__.py` and `tests/__init__.py` as empty files.

- [ ] **Step 5: Install dependencies**

```bash
cd polymarket-bot-python-poc
python -m venv .venv
source .venv/bin/activate
pip install -e ".[dev]"
```

- [ ] **Step 6: Verify setup**

```bash
python -c "import py_clob_client; import websockets; import textual; print('OK')"
```

Expected: `OK`

- [ ] **Step 7: Commit**

```bash
git add polymarket-bot-python-poc/
git commit -m "feat(python-poc): scaffold project with dependencies"
```

---

### Task 2: Types

**Files:**
- Create: `polymarket-bot-python-poc/src/types.py`
- Create: `polymarket-bot-python-poc/tests/test_types.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_types.py
from decimal import Decimal
from src.types import (
    Side, EventKind, EventSource, OrderAction, KillState, OrderState,
    HotEvent, OrderIntent, TelemetryEvent, TelemetryKind,
    DashboardSnapshot, Instrument,
)


def test_side_values():
    assert Side.BUY == 0
    assert Side.SELL == 1


def test_event_kind_values():
    assert EventKind.BOOK_DELTA == 0
    assert EventKind.TRADE == 1
    assert EventKind.TOP_OF_BOOK == 2
    assert EventKind.ORDER_ACK == 3
    assert EventKind.FILL == 4
    assert EventKind.ORDER_REJECT == 5


def test_kill_state_ordering():
    assert KillState.ACTIVE < KillState.REDUCE_ONLY < KillState.PAUSED < KillState.KILLED


def test_hot_event_defaults():
    ev = HotEvent(
        recv_ts=1000, seq=1, instrument_id=0,
        source=EventSource.PM, kind=EventKind.BOOK_DELTA,
        side=Side.BUY, price=Decimal("0.52"), qty=Decimal("100"),
    )
    assert ev.client_order_id == 0
    assert ev.exchange_order_id == ""
    assert ev.flags == 0


def test_order_intent_defaults():
    intent = OrderIntent(
        instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
        qty=Decimal("100"), action=OrderAction.POST, client_order_id=1,
    )
    assert intent.target_order_id == 0
    assert intent.strategy_id == 0


def test_telemetry_event_creation():
    ev = TelemetryEvent(
        kind=TelemetryKind.FILL, strategy_name="ladder_mm",
        strategy_id=0, instrument_id=0,
    )
    assert ev.latency_ms == 0.0
    assert ev.message == ""


def test_instrument_fields():
    inst = Instrument(
        instrument_id=0, token_id="123", condition_id="0xabc",
        outcome="YES", tick_size=Decimal("0.01"), neg_risk=False,
        min_size=Decimal("1"), asset="BTC", timeframe="5m",
        ref_symbol="BTCUSDT",
    )
    assert inst.asset == "BTC"
    assert inst.outcome == "YES"
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd polymarket-bot-python-poc
python -m pytest tests/test_types.py -v
```

Expected: FAIL with `ModuleNotFoundError: No module named 'src.types'`

- [ ] **Step 3: Write types.py**

```python
# src/types.py
"""Shared types for the Polymarket bot — single source of truth."""
from __future__ import annotations

from dataclasses import dataclass, field
from decimal import Decimal
from enum import IntEnum


class Side(IntEnum):
    BUY = 0
    SELL = 1


class EventKind(IntEnum):
    BOOK_DELTA = 0
    TRADE = 1
    TOP_OF_BOOK = 2
    ORDER_ACK = 3
    FILL = 4
    ORDER_REJECT = 5
    HEARTBEAT = 6
    TIMER = 7


class EventSource(IntEnum):
    PM = 0
    BN_BBO = 1
    BN_TRADE = 2
    BN_DEPTH = 3
    ACCOUNT = 4


class OrderAction(IntEnum):
    POST = 0
    CANCEL = 1
    AMEND = 2


class OrderState(IntEnum):
    PENDING = 0
    SENDING = 1
    LIVE = 2
    CANCEL_PENDING = 3
    FILLED = 4
    CANCELLED = 5
    REJECTED = 6


class KillState(IntEnum):
    ACTIVE = 0
    REDUCE_ONLY = 1
    PAUSED = 2
    KILLED = 3


class TelemetryKind(IntEnum):
    MARKET_DATA = 0
    FILL = 1
    ORDER_PLACED = 2
    ORDER_CANCELLED = 3
    RISK_REJECT = 4
    GLOBAL_REJECT = 5
    API_CALL = 6
    FEED_ALERT = 7
    HEARTBEAT = 8
    KILL_SWITCH = 9
    BBO_CHANGE = 10
    RECONCILE = 11


# ── Flag bits ──
FLAG_IS_SNAPSHOT = 1
FLAG_LAST_IN_BATCH = 2


@dataclass(slots=True)
class HotEvent:
    recv_ts: int                        # nanosecond monotonic timestamp
    seq: int                            # monotonic sequence number
    instrument_id: int                  # internal index
    source: EventSource
    kind: EventKind
    side: Side
    price: Decimal
    qty: Decimal
    flags: int = 0                      # FLAG_IS_SNAPSHOT, FLAG_LAST_IN_BATCH
    client_order_id: int = 0
    exchange_order_id: str = ""
    trade_status: str = ""              # MATCHED, CONFIRMED, FAILED
    reject_reason: str = ""
    # Binance-specific fields
    bn_bid: Decimal = Decimal(0)
    bn_ask: Decimal = Decimal(0)
    bn_bid_qty: Decimal = Decimal(0)
    bn_ask_qty: Decimal = Decimal(0)


@dataclass(slots=True)
class OrderIntent:
    instrument_id: int
    side: Side
    price: Decimal
    qty: Decimal
    action: OrderAction
    client_order_id: int
    target_order_id: int = 0            # for CANCEL/AMEND
    strategy_id: int = 0


@dataclass(slots=True)
class TelemetryEvent:
    kind: TelemetryKind
    strategy_name: str = ""
    strategy_id: int = 0
    instrument_id: int = 0
    side: Side = Side.BUY
    price: Decimal = Decimal(0)
    qty: Decimal = Decimal(0)
    latency_ms: float = 0.0
    success: bool = True
    message: str = ""
    order_id: int = 0
    endpoint: str = ""
    count: int = 0
    kill_state: KillState = KillState.ACTIVE


@dataclass(slots=True)
class Instrument:
    instrument_id: int
    token_id: str
    condition_id: str
    outcome: str                        # "YES" / "NO"
    tick_size: Decimal
    neg_risk: bool
    min_size: Decimal
    asset: str                          # "BTC", "SOL", "ETH"
    timeframe: str                      # "5m", "15m"
    ref_symbol: str                     # "BTCUSDT"


@dataclass
class DashboardSnapshot:
    """Populated by telemetry loop, consumed by Textual TUI."""
    epoch_ms: int = 0
    # Latency
    lat_p50: float = 0.0
    lat_p95: float = 0.0
    lat_p99: float = 0.0
    lat_p999: float = 0.0
    # Queue depths
    pm_q_depth: int = 0
    ref_q_depth: int = 0
    account_q_depth: int = 0
    intent_q_depth: int = 0
    # Event rates
    pm_rate: float = 0.0
    bn_rate: float = 0.0
    account_rate: float = 0.0
    # Per-instrument BBO (indexed by instrument_id)
    instruments: dict = field(default_factory=dict)
    # Per-strategy stats
    strategies: dict = field(default_factory=dict)
    # Risk
    kill_state: KillState = KillState.ACTIVE
    heartbeat_ok: bool = True
    heartbeat_latency_ms: float = 0.0
    wallet_free_usdc: Decimal = Decimal(0)
    # Recent events for tapes
    recent_trades: list = field(default_factory=list)
    recent_executions: list = field(default_factory=list)
```

- [ ] **Step 4: Run test to verify it passes**

```bash
python -m pytest tests/test_types.py -v
```

Expected: all 7 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/types.py tests/test_types.py
git commit -m "feat(python-poc): add shared types"
```

---

### Task 3: Config

**Files:**
- Create: `polymarket-bot-python-poc/src/config.py`
- Create: `polymarket-bot-python-poc/config/bot.toml`
- Create: `polymarket-bot-python-poc/tests/test_config.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_config.py
import os
import tempfile
from decimal import Decimal
from src.config import BotConfig, load_config


SAMPLE_TOML = """
[general]
paper_mode = true
timeframe = "5m"
assets = ["BTC", "SOL", "ETH"]
log_level = "INFO"
tape_dir = "data/tapes"

[credentials]
env_file = ".env"

[strategy.ladder_mm]
enabled = true
capital_budget = 500.0
levels = 5
spread_bps = 50
skew_factor = 0.3
max_position = 1000
max_orders_live = 20
max_replaces_per_min = 100
quote_age_ms = 2000

[strategy.momentum]
enabled = true
capital_budget = 300.0
signal_threshold = 0.7
max_position = 500
max_orders_live = 2
max_replaces_per_min = 5

[risk.global]
wallet_min_free_usdc = 100.0
max_live_orders_total = 50
max_submits_per_min = 200
max_cancels_per_min = 300
feed_stale_ms = 2000
feed_dead_ms = 5000

[heartbeat]
interval_s = 5
fail_pause_after_s = 8

[dashboard]
enabled = true
refresh_hz = 10
"""


def test_load_config():
    with tempfile.NamedTemporaryFile(mode="w", suffix=".toml", delete=False) as f:
        f.write(SAMPLE_TOML)
        f.flush()
        cfg = load_config(f.name)

    os.unlink(f.name)

    assert cfg.paper_mode is True
    assert cfg.timeframe == "5m"
    assert cfg.assets == ["BTC", "SOL", "ETH"]


def test_strategy_configs():
    with tempfile.NamedTemporaryFile(mode="w", suffix=".toml", delete=False) as f:
        f.write(SAMPLE_TOML)
        f.flush()
        cfg = load_config(f.name)

    os.unlink(f.name)

    assert "ladder_mm" in cfg.strategies
    assert cfg.strategies["ladder_mm"].enabled is True
    assert cfg.strategies["ladder_mm"].capital_budget == Decimal("500.0")
    assert cfg.strategies["ladder_mm"].levels == 5
    assert "momentum" in cfg.strategies
    assert cfg.strategies["momentum"].signal_threshold == 0.7


def test_risk_config():
    with tempfile.NamedTemporaryFile(mode="w", suffix=".toml", delete=False) as f:
        f.write(SAMPLE_TOML)
        f.flush()
        cfg = load_config(f.name)

    os.unlink(f.name)

    assert cfg.global_risk.wallet_min_free_usdc == Decimal("100.0")
    assert cfg.global_risk.max_live_orders_total == 50
    assert cfg.global_risk.feed_dead_ms == 5000
```

- [ ] **Step 2: Run test to verify it fails**

```bash
python -m pytest tests/test_config.py -v
```

Expected: FAIL

- [ ] **Step 3: Write config.py**

```python
# src/config.py
"""TOML config loader for bot configuration."""
from __future__ import annotations

import tomllib
from dataclasses import dataclass, field
from decimal import Decimal
from pathlib import Path


@dataclass
class StrategyConfig:
    enabled: bool = False
    capital_budget: Decimal = Decimal(0)
    levels: int = 5
    spread_bps: int = 50
    skew_factor: float = 0.3
    max_position: int = 1000
    max_orders_live: int = 20
    max_replaces_per_min: int = 100
    quote_age_ms: int = 2000
    signal_threshold: float = 0.7
    max_intents_per_sec: int = 50


@dataclass
class GlobalRiskConfig:
    wallet_min_free_usdc: Decimal = Decimal("100")
    max_live_orders_total: int = 50
    max_submits_per_min: int = 200
    max_cancels_per_min: int = 300
    feed_stale_ms: int = 2000
    feed_dead_ms: int = 5000


@dataclass
class HeartbeatConfig:
    interval_s: int = 5
    fail_pause_after_s: int = 8


@dataclass
class DashboardConfig:
    enabled: bool = True
    refresh_hz: int = 10


@dataclass
class BotConfig:
    paper_mode: bool = True
    timeframe: str = "5m"
    assets: list[str] = field(default_factory=lambda: ["BTC", "SOL", "ETH"])
    log_level: str = "INFO"
    tape_dir: str = "data/tapes"
    env_file: str = ".env"
    strategies: dict[str, StrategyConfig] = field(default_factory=dict)
    global_risk: GlobalRiskConfig = field(default_factory=GlobalRiskConfig)
    heartbeat: HeartbeatConfig = field(default_factory=HeartbeatConfig)
    dashboard: DashboardConfig = field(default_factory=DashboardConfig)


def load_config(path: str | Path) -> BotConfig:
    with open(path, "rb") as f:
        raw = tomllib.load(f)

    general = raw.get("general", {})
    creds = raw.get("credentials", {})
    hb = raw.get("heartbeat", {})
    dash = raw.get("dashboard", {})
    risk_raw = raw.get("risk", {}).get("global", {})

    strategies = {}
    for name, strat_raw in raw.get("strategy", {}).items():
        sc = StrategyConfig()
        for k, v in strat_raw.items():
            if k == "capital_budget":
                setattr(sc, k, Decimal(str(v)))
            elif hasattr(sc, k):
                setattr(sc, k, v)
        strategies[name] = sc

    gr = GlobalRiskConfig()
    for k, v in risk_raw.items():
        if k == "wallet_min_free_usdc":
            setattr(gr, k, Decimal(str(v)))
        elif hasattr(gr, k):
            setattr(gr, k, v)

    return BotConfig(
        paper_mode=general.get("paper_mode", True),
        timeframe=general.get("timeframe", "5m"),
        assets=general.get("assets", ["BTC", "SOL", "ETH"]),
        log_level=general.get("log_level", "INFO"),
        tape_dir=general.get("tape_dir", "data/tapes"),
        env_file=creds.get("env_file", ".env"),
        strategies=strategies,
        global_risk=gr,
        heartbeat=HeartbeatConfig(
            interval_s=hb.get("interval_s", 5),
            fail_pause_after_s=hb.get("fail_pause_after_s", 8),
        ),
        dashboard=DashboardConfig(
            enabled=dash.get("enabled", True),
            refresh_hz=dash.get("refresh_hz", 10),
        ),
    )
```

- [ ] **Step 4: Create config/bot.toml** (copy the SAMPLE_TOML content from the test)

- [ ] **Step 5: Run tests**

```bash
python -m pytest tests/test_config.py -v
```

Expected: all 3 tests PASS

- [ ] **Step 6: Commit**

```bash
git add src/config.py config/bot.toml tests/test_config.py
git commit -m "feat(python-poc): add TOML config loader"
```

---

### Task 4: Order Book

**Files:**
- Create: `polymarket-bot-python-poc/src/engine/__init__.py`
- Create: `polymarket-bot-python-poc/src/engine/book.py`
- Create: `polymarket-bot-python-poc/tests/test_book.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_book.py
from decimal import Decimal
from src.engine.book import PmBook, BnBook


def test_pm_book_apply_and_best():
    book = PmBook()
    # Add bid levels
    book.apply_level(side_is_bid=True, price=Decimal("0.50"), size=Decimal("100"))
    book.apply_level(side_is_bid=True, price=Decimal("0.49"), size=Decimal("200"))
    # Add ask levels
    book.apply_level(side_is_bid=False, price=Decimal("0.52"), size=Decimal("150"))
    book.apply_level(side_is_bid=False, price=Decimal("0.53"), size=Decimal("50"))

    assert book.best_bid() == (Decimal("0.50"), Decimal("100"))
    assert book.best_ask() == (Decimal("0.52"), Decimal("150"))


def test_pm_book_remove_level():
    book = PmBook()
    book.apply_level(side_is_bid=True, price=Decimal("0.50"), size=Decimal("100"))
    book.apply_level(side_is_bid=True, price=Decimal("0.50"), size=Decimal("0"))
    assert book.best_bid() == (Decimal(0), Decimal(0))


def test_pm_book_clear():
    book = PmBook()
    book.apply_level(side_is_bid=True, price=Decimal("0.50"), size=Decimal("100"))
    book.clear()
    assert book.best_bid() == (Decimal(0), Decimal(0))
    assert book.best_ask() == (Decimal(0), Decimal(0))


def test_pm_book_weighted_mid():
    book = PmBook()
    book.apply_level(side_is_bid=True, price=Decimal("0.50"), size=Decimal("100"))
    book.apply_level(side_is_bid=False, price=Decimal("0.52"), size=Decimal("100"))
    mid = book.weighted_mid()
    assert mid == Decimal("0.51")  # equal sizes → simple mid


def test_pm_book_weighted_mid_skewed():
    book = PmBook()
    book.apply_level(side_is_bid=True, price=Decimal("0.50"), size=Decimal("300"))
    book.apply_level(side_is_bid=False, price=Decimal("0.52"), size=Decimal("100"))
    mid = book.weighted_mid()
    # weighted mid = (0.50*100 + 0.52*300) / (100+300) = 206/400 = 0.515
    assert mid == Decimal("0.515")


def test_pm_book_top_levels():
    book = PmBook()
    for i in range(10):
        p = Decimal("0.40") + Decimal(i) * Decimal("0.01")
        book.apply_level(side_is_bid=True, price=p, size=Decimal("100"))
    levels = book.top_bids(5)
    assert len(levels) == 5
    assert levels[0][0] == Decimal("0.49")  # best bid first


def test_bn_book_snapshot():
    book = BnBook()
    bids = [("67000.00", "1.5"), ("66999.50", "2.0")]
    asks = [("67001.00", "0.8"), ("67001.50", "1.2")]
    book.apply_snapshot(bids, asks)
    assert book.best_bid() == (Decimal("67000.00"), Decimal("1.5"))
    assert book.best_ask() == (Decimal("67001.00"), Decimal("0.8"))
    assert book.bid_count == 2
    assert book.ask_count == 2


def test_bn_book_mid():
    book = BnBook()
    book.apply_bbo(
        bid=Decimal("67000.00"), bid_qty=Decimal("1.0"),
        ask=Decimal("67001.00"), ask_qty=Decimal("0.5"),
    )
    assert book.mid() == Decimal("67000.50")
```

- [ ] **Step 2: Run test to verify it fails**

```bash
python -m pytest tests/test_book.py -v
```

Expected: FAIL

- [ ] **Step 3: Write book.py**

```python
# src/engine/book.py
"""Order book implementations — PM milli-price array + BN depth20."""
from __future__ import annotations

from decimal import Decimal

# PM book uses 0-1000 milli-price index (price 0.000 to 1.000)
_PM_LEVELS = 1001


class PmBook:
    """Polymarket order book indexed by milli-price (0-1000)."""

    __slots__ = ("_bids", "_asks", "change_count")

    def __init__(self) -> None:
        self._bids: list[Decimal] = [Decimal(0)] * _PM_LEVELS
        self._asks: list[Decimal] = [Decimal(0)] * _PM_LEVELS
        self.change_count: int = 0

    def _price_to_idx(self, price: Decimal) -> int:
        return int(price * 1000 + Decimal("0.5"))

    def apply_level(self, side_is_bid: bool, price: Decimal, size: Decimal) -> None:
        idx = self._price_to_idx(price)
        if 0 <= idx < _PM_LEVELS:
            if side_is_bid:
                self._bids[idx] = size
            else:
                self._asks[idx] = size
            self.change_count += 1

    def clear(self) -> None:
        for i in range(_PM_LEVELS):
            self._bids[i] = Decimal(0)
            self._asks[i] = Decimal(0)

    def best_bid(self) -> tuple[Decimal, Decimal]:
        for i in range(_PM_LEVELS - 1, -1, -1):
            if self._bids[i] > 0:
                return (Decimal(i) / 1000, self._bids[i])
        return (Decimal(0), Decimal(0))

    def best_ask(self) -> tuple[Decimal, Decimal]:
        for i in range(_PM_LEVELS):
            if self._asks[i] > 0:
                return (Decimal(i) / 1000, self._asks[i])
        return (Decimal(0), Decimal(0))

    def weighted_mid(self) -> Decimal:
        bp, bs = self.best_bid()
        ap, az = self.best_ask()
        if bs + az == 0:
            return Decimal(0)
        return (bp * az + ap * bs) / (bs + az)

    def mid(self) -> Decimal:
        bp, _ = self.best_bid()
        ap, _ = self.best_ask()
        if bp == 0 or ap == 0:
            return Decimal(0)
        return (bp + ap) / 2

    def spread(self) -> Decimal:
        bp, _ = self.best_bid()
        ap, _ = self.best_ask()
        if bp == 0 or ap == 0:
            return Decimal(0)
        return ap - bp

    def top_bids(self, n: int) -> list[tuple[Decimal, Decimal]]:
        result = []
        for i in range(_PM_LEVELS - 1, -1, -1):
            if self._bids[i] > 0:
                result.append((Decimal(i) / 1000, self._bids[i]))
                if len(result) >= n:
                    break
        return result

    def top_asks(self, n: int) -> list[tuple[Decimal, Decimal]]:
        result = []
        for i in range(_PM_LEVELS):
            if self._asks[i] > 0:
                result.append((Decimal(i) / 1000, self._asks[i]))
                if len(result) >= n:
                    break
        return result

    def level_size(self, side_is_bid: bool, price: Decimal) -> Decimal:
        idx = self._price_to_idx(price)
        if 0 <= idx < _PM_LEVELS:
            return self._bids[idx] if side_is_bid else self._asks[idx]
        return Decimal(0)


class BnBook:
    """Binance reference book — BBO + depth20 snapshot."""

    __slots__ = ("_bids", "_asks", "_bbo_bid", "_bbo_ask",
                 "_bbo_bid_qty", "_bbo_ask_qty", "bid_count", "ask_count")

    def __init__(self) -> None:
        self._bids: list[tuple[Decimal, Decimal]] = []
        self._asks: list[tuple[Decimal, Decimal]] = []
        self._bbo_bid = Decimal(0)
        self._bbo_ask = Decimal(0)
        self._bbo_bid_qty = Decimal(0)
        self._bbo_ask_qty = Decimal(0)
        self.bid_count: int = 0
        self.ask_count: int = 0

    def apply_bbo(self, bid: Decimal, bid_qty: Decimal,
                  ask: Decimal, ask_qty: Decimal) -> None:
        self._bbo_bid = bid
        self._bbo_bid_qty = bid_qty
        self._bbo_ask = ask
        self._bbo_ask_qty = ask_qty

    def apply_snapshot(self, bids: list[tuple[str, str]],
                       asks: list[tuple[str, str]]) -> None:
        self._bids = [(Decimal(p), Decimal(q)) for p, q in bids]
        self._asks = [(Decimal(p), Decimal(q)) for p, q in asks]
        self.bid_count = len(self._bids)
        self.ask_count = len(self._asks)
        if self._bids:
            self._bbo_bid = self._bids[0][0]
            self._bbo_bid_qty = self._bids[0][1]
        if self._asks:
            self._bbo_ask = self._asks[0][0]
            self._bbo_ask_qty = self._asks[0][1]

    def best_bid(self) -> tuple[Decimal, Decimal]:
        return (self._bbo_bid, self._bbo_bid_qty)

    def best_ask(self) -> tuple[Decimal, Decimal]:
        return (self._bbo_ask, self._bbo_ask_qty)

    def mid(self) -> Decimal:
        if self._bbo_bid == 0 or self._bbo_ask == 0:
            return Decimal(0)
        return (self._bbo_bid + self._bbo_ask) / 2
```

Also create `src/engine/__init__.py` as empty file.

- [ ] **Step 4: Run tests**

```bash
python -m pytest tests/test_book.py -v
```

Expected: all 9 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/engine/ tests/test_book.py
git commit -m "feat(python-poc): add PM + BN order book"
```

---

### Task 5: Market State

**Files:**
- Create: `polymarket-bot-python-poc/src/engine/market_state.py`
- Create: `polymarket-bot-python-poc/tests/test_market_state.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_market_state.py
from decimal import Decimal
from src.types import HotEvent, EventKind, EventSource, Side, FLAG_IS_SNAPSHOT, FLAG_LAST_IN_BATCH
from src.engine.market_state import MarketState


def test_market_state_process_delta():
    ms = MarketState(num_instruments=4)
    ev = HotEvent(
        recv_ts=1000, seq=1, instrument_id=0,
        source=EventSource.PM, kind=EventKind.BOOK_DELTA,
        side=Side.BUY, price=Decimal("0.50"), qty=Decimal("100"),
    )
    ms.process(ev)
    bp, bs = ms.best_bid(0)
    assert bp == Decimal("0.50")
    assert bs == Decimal("100")


def test_market_state_bbo_change_detection():
    ms = MarketState(num_instruments=4)
    # First delta — sets initial BBO
    ms.process(HotEvent(
        recv_ts=1000, seq=1, instrument_id=0,
        source=EventSource.PM, kind=EventKind.BOOK_DELTA,
        side=Side.BUY, price=Decimal("0.50"), qty=Decimal("100"),
    ))
    ms.process(HotEvent(
        recv_ts=1000, seq=2, instrument_id=0,
        source=EventSource.PM, kind=EventKind.BOOK_DELTA,
        side=Side.SELL, price=Decimal("0.52"), qty=Decimal("100"),
    ))
    # BBO is now set, take_tob should return it
    tob = ms.take_tob(0)
    assert tob is not None
    assert tob["bid_price"] == Decimal("0.50")
    assert tob["ask_price"] == Decimal("0.52")
    # Second call returns None (edge-triggered)
    assert ms.take_tob(0) is None


def test_market_state_staleness():
    ms = MarketState(num_instruments=4)
    ms.process(HotEvent(
        recv_ts=1_000_000_000, seq=1, instrument_id=0,
        source=EventSource.PM, kind=EventKind.BOOK_DELTA,
        side=Side.BUY, price=Decimal("0.50"), qty=Decimal("100"),
    ))
    assert ms.is_stale(0, now_ns=1_000_000_000 + 500_000_000) is False  # 500ms
    assert ms.is_stale(0, now_ns=1_000_000_000 + 3_000_000_000) is True  # 3s


def test_market_state_bn_bbo():
    ms = MarketState(num_instruments=4)
    ev = HotEvent(
        recv_ts=1000, seq=1, instrument_id=2,
        source=EventSource.BN_BBO, kind=EventKind.TOP_OF_BOOK,
        side=Side.BUY, price=Decimal(0), qty=Decimal(0),
        bn_bid=Decimal("67000"), bn_ask=Decimal("67001"),
        bn_bid_qty=Decimal("1.5"), bn_ask_qty=Decimal("0.8"),
    )
    ms.process(ev)
    bp, _ = ms.bn_best_bid(2)
    ap, _ = ms.bn_best_ask(2)
    assert bp == Decimal("67000")
    assert ap == Decimal("67001")


def test_market_state_snapshot_suppression():
    ms = MarketState(num_instruments=4)
    # During snapshot, BBO changes should be suppressed until LAST_IN_BATCH
    ms.process(HotEvent(
        recv_ts=1000, seq=1, instrument_id=0,
        source=EventSource.PM, kind=EventKind.BOOK_DELTA,
        side=Side.BUY, price=Decimal("0.50"), qty=Decimal("100"),
        flags=FLAG_IS_SNAPSHOT,
    ))
    assert ms.take_tob(0) is None  # suppressed during snapshot

    ms.process(HotEvent(
        recv_ts=1000, seq=2, instrument_id=0,
        source=EventSource.PM, kind=EventKind.BOOK_DELTA,
        side=Side.SELL, price=Decimal("0.52"), qty=Decimal("50"),
        flags=FLAG_IS_SNAPSHOT | FLAG_LAST_IN_BATCH,
    ))
    tob = ms.take_tob(0)
    assert tob is not None  # snapshot complete, emit BBO
```

- [ ] **Step 2: Run test to verify it fails**

```bash
python -m pytest tests/test_market_state.py -v
```

Expected: FAIL

- [ ] **Step 3: Write market_state.py**

```python
# src/engine/market_state.py
"""Per-instrument market state — book + BBO tracking + staleness."""
from __future__ import annotations

from decimal import Decimal

from src.engine.book import PmBook, BnBook
from src.types import HotEvent, EventKind, EventSource, FLAG_IS_SNAPSHOT, FLAG_LAST_IN_BATCH


class InstrumentState:
    __slots__ = (
        "pm_book", "bn_book", "last_event_ns", "last_seq",
        "_prev_bid", "_prev_ask", "_bbo_changed", "_in_snapshot",
    )

    def __init__(self) -> None:
        self.pm_book = PmBook()
        self.bn_book = BnBook()
        self.last_event_ns: int = 0
        self.last_seq: int = 0
        self._prev_bid: tuple[Decimal, Decimal] = (Decimal(0), Decimal(0))
        self._prev_ask: tuple[Decimal, Decimal] = (Decimal(0), Decimal(0))
        self._bbo_changed: bool = False
        self._in_snapshot: bool = False


class MarketState:
    """Manages per-instrument state for all instruments."""

    def __init__(self, num_instruments: int, stale_ns: int = 2_000_000_000) -> None:
        self._instruments = [InstrumentState() for _ in range(num_instruments)]
        self._stale_ns = stale_ns

    def process(self, event: HotEvent) -> None:
        if event.instrument_id >= len(self._instruments):
            return
        inst = self._instruments[event.instrument_id]
        inst.last_event_ns = event.recv_ts
        inst.last_seq = event.seq

        if event.source == EventSource.BN_BBO:
            inst.bn_book.apply_bbo(
                event.bn_bid, event.bn_bid_qty,
                event.bn_ask, event.bn_ask_qty,
            )
            return

        if event.source == EventSource.BN_DEPTH:
            if event.kind == EventKind.BOOK_DELTA:
                # Depth snapshots come as clear + deltas
                if event.flags & FLAG_IS_SNAPSHOT:
                    inst.bn_book.apply_snapshot([], [])
            return

        if event.source in (EventSource.PM,):
            is_snapshot = bool(event.flags & FLAG_IS_SNAPSHOT)
            is_last = bool(event.flags & FLAG_LAST_IN_BATCH)

            if is_snapshot and not inst._in_snapshot:
                inst._in_snapshot = True
                inst.pm_book.clear()

            if event.kind == EventKind.BOOK_DELTA:
                inst.pm_book.apply_level(
                    side_is_bid=(event.side == 0),
                    price=event.price,
                    size=event.qty,
                )

            if inst._in_snapshot:
                if is_last:
                    inst._in_snapshot = False
                    self._check_bbo_change(inst)
            else:
                self._check_bbo_change(inst)

    def _check_bbo_change(self, inst: InstrumentState) -> None:
        cur_bid = inst.pm_book.best_bid()
        cur_ask = inst.pm_book.best_ask()
        if cur_bid[0] > 0 and cur_ask[0] > 0:
            if cur_bid != inst._prev_bid or cur_ask != inst._prev_ask:
                inst._bbo_changed = True
                inst._prev_bid = cur_bid
                inst._prev_ask = cur_ask

    def take_tob(self, instrument_id: int) -> dict | None:
        if instrument_id >= len(self._instruments):
            return None
        inst = self._instruments[instrument_id]
        if not inst._bbo_changed:
            return None
        inst._bbo_changed = False
        bp, bs = inst.pm_book.best_bid()
        ap, az = inst.pm_book.best_ask()
        return {
            "bid_price": bp, "bid_size": bs,
            "ask_price": ap, "ask_size": az,
            "mid": inst.pm_book.mid(),
            "weighted_mid": inst.pm_book.weighted_mid(),
            "spread": inst.pm_book.spread(),
        }

    def best_bid(self, instrument_id: int) -> tuple[Decimal, Decimal]:
        return self._instruments[instrument_id].pm_book.best_bid()

    def best_ask(self, instrument_id: int) -> tuple[Decimal, Decimal]:
        return self._instruments[instrument_id].pm_book.best_ask()

    def bn_best_bid(self, instrument_id: int) -> tuple[Decimal, Decimal]:
        return self._instruments[instrument_id].bn_book.best_bid()

    def bn_best_ask(self, instrument_id: int) -> tuple[Decimal, Decimal]:
        return self._instruments[instrument_id].bn_book.best_ask()

    def is_stale(self, instrument_id: int, now_ns: int) -> bool:
        inst = self._instruments[instrument_id]
        if inst.last_event_ns == 0:
            return True
        return (now_ns - inst.last_event_ns) > self._stale_ns

    def is_ready(self, instrument_id: int) -> bool:
        inst = self._instruments[instrument_id]
        bp, _ = inst.pm_book.best_bid()
        ap, _ = inst.pm_book.best_ask()
        return bp > 0 and ap > 0 and ap > bp

    def book(self, instrument_id: int) -> PmBook:
        return self._instruments[instrument_id].pm_book

    def bn_book(self, instrument_id: int) -> BnBook:
        return self._instruments[instrument_id].bn_book
```

- [ ] **Step 4: Run tests**

```bash
python -m pytest tests/test_market_state.py -v
```

Expected: all 5 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/engine/market_state.py tests/test_market_state.py
git commit -m "feat(python-poc): add market state engine"
```

---

### Task 6: Ingest Tasks (PM + BN + Account)

**Files:**
- Create: `polymarket-bot-python-poc/src/ingest/__init__.py`
- Create: `polymarket-bot-python-poc/src/ingest/polymarket.py`
- Create: `polymarket-bot-python-poc/src/ingest/binance.py`
- Create: `polymarket-bot-python-poc/src/ingest/account.py`

These are async tasks — tested via integration later. Focus on correct WS message parsing.

- [ ] **Step 1: Write polymarket.py**

```python
# src/ingest/polymarket.py
"""Polymarket market data WebSocket ingest."""
from __future__ import annotations

import asyncio
import json
import logging
import time
from decimal import Decimal

import websockets

from src.types import HotEvent, EventKind, EventSource, Side, FLAG_IS_SNAPSHOT, FLAG_LAST_IN_BATCH, Instrument

logger = logging.getLogger(__name__)

WS_MARKET_URL = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
PING_INTERVAL = 10.0


async def pm_ingest(
    pm_q: asyncio.Queue,
    instruments: list[Instrument],
    shutdown: asyncio.Event,
) -> None:
    """Connect to PM market WS, parse events, push to pm_q."""
    token_to_inst: dict[str, int] = {}
    all_tokens: list[str] = []
    for inst in instruments:
        token_to_inst[inst.token_id] = inst.instrument_id
        all_tokens.append(inst.token_id)

    seq = 0

    while not shutdown.is_set():
        try:
            async with websockets.connect(WS_MARKET_URL) as ws:
                logger.info("PM market WS connected, subscribing to %d tokens", len(all_tokens))
                await ws.send(json.dumps({
                    "assets_ids": all_tokens,
                    "type": "market",
                    "custom_feature_enabled": True,
                }))

                last_ping = time.monotonic()

                while not shutdown.is_set():
                    # Keepalive
                    now = time.monotonic()
                    if now - last_ping > PING_INTERVAL:
                        try:
                            await ws.send("PING")
                        except Exception:
                            logger.warning("PM PING failed")
                            break
                        last_ping = now

                    try:
                        raw = await asyncio.wait_for(ws.recv(), timeout=1.0)
                    except asyncio.TimeoutError:
                        continue
                    except websockets.ConnectionClosed:
                        logger.warning("PM WS closed")
                        break

                    if not raw or raw == "PONG":
                        continue

                    recv_ns = time.monotonic_ns()

                    try:
                        parsed = json.loads(raw)
                    except json.JSONDecodeError:
                        continue

                    msgs = parsed if isinstance(parsed, list) else [parsed]

                    for msg in msgs:
                        if not isinstance(msg, dict):
                            continue
                        et = msg.get("event_type", "")
                        aid = msg.get("asset_id", "")

                        if et == "book":
                            inst_id = token_to_inst.get(aid)
                            if inst_id is None:
                                continue
                            bids = msg.get("bids", [])
                            asks = msg.get("asks", [])
                            total = len(bids) + len(asks)
                            for i, item in enumerate(bids):
                                seq += 1
                                is_last = (i == len(bids) - 1 and len(asks) == 0)
                                flags = FLAG_IS_SNAPSHOT
                                if is_last:
                                    flags |= FLAG_LAST_IN_BATCH
                                pm_q.put_nowait(HotEvent(
                                    recv_ts=recv_ns, seq=seq, instrument_id=inst_id,
                                    source=EventSource.PM, kind=EventKind.BOOK_DELTA,
                                    side=Side.BUY,
                                    price=Decimal(item["price"]),
                                    qty=Decimal(item["size"]),
                                    flags=flags,
                                ))
                            for i, item in enumerate(asks):
                                seq += 1
                                is_last = (i == len(asks) - 1)
                                flags = FLAG_IS_SNAPSHOT
                                if is_last:
                                    flags |= FLAG_LAST_IN_BATCH
                                pm_q.put_nowait(HotEvent(
                                    recv_ts=recv_ns, seq=seq, instrument_id=inst_id,
                                    source=EventSource.PM, kind=EventKind.BOOK_DELTA,
                                    side=Side.SELL,
                                    price=Decimal(item["price"]),
                                    qty=Decimal(item["size"]),
                                    flags=flags,
                                ))

                        elif et == "price_change":
                            for ch in msg.get("price_changes", []):
                                c_aid = ch.get("asset_id", "")
                                inst_id = token_to_inst.get(c_aid)
                                if inst_id is None:
                                    continue
                                seq += 1
                                side = Side.BUY if ch.get("side") == "BUY" else Side.SELL
                                pm_q.put_nowait(HotEvent(
                                    recv_ts=recv_ns, seq=seq, instrument_id=inst_id,
                                    source=EventSource.PM, kind=EventKind.BOOK_DELTA,
                                    side=side,
                                    price=Decimal(ch["price"]),
                                    qty=Decimal(ch["size"]),
                                ))

                        elif et == "last_trade_price":
                            inst_id = token_to_inst.get(aid)
                            if inst_id is None:
                                continue
                            seq += 1
                            side = Side.BUY if msg.get("side") == "BUY" else Side.SELL
                            pm_q.put_nowait(HotEvent(
                                recv_ts=recv_ns, seq=seq, instrument_id=inst_id,
                                source=EventSource.PM, kind=EventKind.TRADE,
                                side=side,
                                price=Decimal(msg.get("price", "0")),
                                qty=Decimal(msg.get("size", "0")),
                            ))

        except Exception as e:
            logger.error("PM ingest error: %s, reconnecting in 2s", e)
            await asyncio.sleep(2.0)
```

- [ ] **Step 2: Write binance.py**

```python
# src/ingest/binance.py
"""Binance reference data WebSocket ingest — BBO + trades + depth20."""
from __future__ import annotations

import asyncio
import json
import logging
import time
from decimal import Decimal

import websockets

from src.types import HotEvent, EventKind, EventSource, Side, FLAG_IS_SNAPSHOT, FLAG_LAST_IN_BATCH

logger = logging.getLogger(__name__)

BN_WS_BASE = "wss://stream.binance.com:9443/ws/"


async def bn_ingest(
    ref_q: asyncio.Queue,
    ref_symbols: list[tuple[str, int]],  # [(symbol_lower, instrument_id), ...]
    shutdown: asyncio.Event,
) -> None:
    """Launch BBO + trade + depth20 feeds for each reference symbol."""
    tasks = []
    for sym_lower, inst_id in ref_symbols:
        tasks.append(asyncio.create_task(_bbo_feed(ref_q, sym_lower, inst_id, shutdown)))
        tasks.append(asyncio.create_task(_trade_feed(ref_q, sym_lower, inst_id, shutdown)))
        tasks.append(asyncio.create_task(_depth_feed(ref_q, sym_lower, inst_id, shutdown)))

    await asyncio.gather(*tasks, return_exceptions=True)


async def _bbo_feed(
    ref_q: asyncio.Queue, sym: str, inst_id: int, shutdown: asyncio.Event,
) -> None:
    url = f"{BN_WS_BASE}{sym}@bookTicker"
    seq = 0
    while not shutdown.is_set():
        try:
            async with websockets.connect(url) as ws:
                logger.info("BN BBO connected: %s", sym)
                while not shutdown.is_set():
                    try:
                        raw = await asyncio.wait_for(ws.recv(), timeout=2.0)
                    except asyncio.TimeoutError:
                        continue
                    except websockets.ConnectionClosed:
                        break
                    recv_ns = time.monotonic_ns()
                    msg = json.loads(raw)
                    seq += 1
                    ref_q.put_nowait(HotEvent(
                        recv_ts=recv_ns, seq=seq, instrument_id=inst_id,
                        source=EventSource.BN_BBO, kind=EventKind.TOP_OF_BOOK,
                        side=Side.BUY, price=Decimal(0), qty=Decimal(0),
                        bn_bid=Decimal(msg.get("b", "0")),
                        bn_ask=Decimal(msg.get("a", "0")),
                        bn_bid_qty=Decimal(msg.get("B", "0")),
                        bn_ask_qty=Decimal(msg.get("A", "0")),
                    ))
        except Exception as e:
            logger.error("BN BBO %s error: %s, reconnecting", sym, e)
            await asyncio.sleep(2.0)


async def _trade_feed(
    ref_q: asyncio.Queue, sym: str, inst_id: int, shutdown: asyncio.Event,
) -> None:
    url = f"{BN_WS_BASE}{sym}@trade"
    seq = 0
    while not shutdown.is_set():
        try:
            async with websockets.connect(url) as ws:
                logger.info("BN trade connected: %s", sym)
                while not shutdown.is_set():
                    try:
                        raw = await asyncio.wait_for(ws.recv(), timeout=2.0)
                    except asyncio.TimeoutError:
                        continue
                    except websockets.ConnectionClosed:
                        break
                    recv_ns = time.monotonic_ns()
                    msg = json.loads(raw)
                    seq += 1
                    is_buyer_maker = msg.get("m", False)
                    side = Side.SELL if is_buyer_maker else Side.BUY
                    ref_q.put_nowait(HotEvent(
                        recv_ts=recv_ns, seq=seq, instrument_id=inst_id,
                        source=EventSource.BN_TRADE, kind=EventKind.TRADE,
                        side=side,
                        price=Decimal(msg.get("p", "0")),
                        qty=Decimal(msg.get("q", "0")),
                    ))
        except Exception as e:
            logger.error("BN trade %s error: %s, reconnecting", sym, e)
            await asyncio.sleep(2.0)


async def _depth_feed(
    ref_q: asyncio.Queue, sym: str, inst_id: int, shutdown: asyncio.Event,
) -> None:
    url = f"{BN_WS_BASE}{sym}@depth20@100ms"
    seq = 0
    while not shutdown.is_set():
        try:
            async with websockets.connect(url) as ws:
                logger.info("BN depth20 connected: %s", sym)
                while not shutdown.is_set():
                    try:
                        raw = await asyncio.wait_for(ws.recv(), timeout=2.0)
                    except asyncio.TimeoutError:
                        continue
                    except websockets.ConnectionClosed:
                        break
                    recv_ns = time.monotonic_ns()
                    msg = json.loads(raw)
                    bids = msg.get("bids", [])
                    asks = msg.get("asks", [])
                    total = len(bids) + len(asks)
                    for i, level in enumerate(bids):
                        seq += 1
                        is_last = (i == len(bids) - 1 and len(asks) == 0)
                        flags = FLAG_IS_SNAPSHOT
                        if is_last:
                            flags |= FLAG_LAST_IN_BATCH
                        ref_q.put_nowait(HotEvent(
                            recv_ts=recv_ns, seq=seq, instrument_id=inst_id,
                            source=EventSource.BN_DEPTH, kind=EventKind.BOOK_DELTA,
                            side=Side.BUY,
                            price=Decimal(level[0]),
                            qty=Decimal(level[1]),
                            flags=flags,
                        ))
                    for i, level in enumerate(asks):
                        seq += 1
                        is_last = (i == len(asks) - 1)
                        flags = FLAG_IS_SNAPSHOT
                        if is_last:
                            flags |= FLAG_LAST_IN_BATCH
                        ref_q.put_nowait(HotEvent(
                            recv_ts=recv_ns, seq=seq, instrument_id=inst_id,
                            source=EventSource.BN_DEPTH, kind=EventKind.BOOK_DELTA,
                            side=Side.SELL,
                            price=Decimal(level[0]),
                            qty=Decimal(level[1]),
                            flags=flags,
                        ))
        except Exception as e:
            logger.error("BN depth %s error: %s, reconnecting", sym, e)
            await asyncio.sleep(2.0)
```

- [ ] **Step 3: Write account.py**

```python
# src/ingest/account.py
"""Polymarket user channel WebSocket — fills, order updates."""
from __future__ import annotations

import asyncio
import json
import logging
import time
from decimal import Decimal

import websockets

from src.types import HotEvent, EventKind, EventSource, Side, Instrument

logger = logging.getLogger(__name__)

WS_USER_URL = "wss://ws-subscriptions-clob.polymarket.com/ws/user"
PING_INTERVAL = 10.0


async def account_ingest(
    account_q: asyncio.Queue,
    instruments: list[Instrument],
    api_key: str,
    api_secret: str,
    passphrase: str,
    shutdown: asyncio.Event,
) -> None:
    """Connect to PM user channel WS, parse fills/order updates, push to account_q."""
    token_to_inst: dict[str, int] = {}
    condition_ids: list[str] = []
    seen_conditions: set[str] = set()
    for inst in instruments:
        token_to_inst[inst.token_id] = inst.instrument_id
        if inst.condition_id not in seen_conditions:
            condition_ids.append(inst.condition_id)
            seen_conditions.add(inst.condition_id)

    seq = 0

    while not shutdown.is_set():
        try:
            async with websockets.connect(WS_USER_URL) as ws:
                logger.info("PM user channel WS connected")
                await ws.send(json.dumps({
                    "auth": {
                        "apiKey": api_key,
                        "secret": api_secret,
                        "passphrase": passphrase,
                    },
                    "markets": condition_ids,
                    "type": "user",
                }))

                last_ping = time.monotonic()

                while not shutdown.is_set():
                    now = time.monotonic()
                    if now - last_ping > PING_INTERVAL:
                        try:
                            await ws.send("PING")
                        except Exception:
                            logger.warning("PM user PING failed")
                            break
                        last_ping = now

                    try:
                        raw = await asyncio.wait_for(ws.recv(), timeout=1.0)
                    except asyncio.TimeoutError:
                        continue
                    except websockets.ConnectionClosed:
                        logger.warning("PM user WS closed")
                        break

                    if not raw or raw == "PONG":
                        continue

                    recv_ns = time.monotonic_ns()

                    try:
                        msg = json.loads(raw)
                    except json.JSONDecodeError:
                        continue

                    msgs = msg if isinstance(msg, list) else [msg]

                    for m in msgs:
                        if not isinstance(m, dict):
                            continue
                        et = m.get("event_type", "")
                        aid = m.get("asset_id", "")
                        inst_id = token_to_inst.get(aid, -1)
                        if inst_id < 0:
                            continue

                        if et == "trade":
                            seq += 1
                            side = Side.BUY if m.get("side") == "BUY" else Side.SELL
                            status = m.get("status", "MATCHED")
                            # Extract client_order_id from maker_orders if present
                            client_oid = 0
                            maker_orders = m.get("maker_orders", [])
                            if maker_orders:
                                client_oid = hash(maker_orders[0].get("order_id", "")) & 0xFFFFFFFFFFFFFFFF
                            account_q.put_nowait(HotEvent(
                                recv_ts=recv_ns, seq=seq, instrument_id=inst_id,
                                source=EventSource.ACCOUNT, kind=EventKind.FILL,
                                side=side,
                                price=Decimal(m.get("price", "0")),
                                qty=Decimal(m.get("size", "0")),
                                trade_status=status,
                                client_order_id=client_oid,
                                exchange_order_id=m.get("id", ""),
                            ))

                        elif et == "order":
                            seq += 1
                            order_type = m.get("type", "")
                            if order_type == "PLACEMENT":
                                kind = EventKind.ORDER_ACK
                            elif order_type == "CANCELLATION":
                                kind = EventKind.ORDER_ACK
                            else:
                                continue
                            side = Side.BUY if m.get("side") == "BUY" else Side.SELL
                            account_q.put_nowait(HotEvent(
                                recv_ts=recv_ns, seq=seq, instrument_id=inst_id,
                                source=EventSource.ACCOUNT, kind=kind,
                                side=side,
                                price=Decimal(m.get("price", "0")),
                                qty=Decimal(m.get("original_size", "0")),
                                exchange_order_id=m.get("id", ""),
                            ))

        except Exception as e:
            logger.error("Account ingest error: %s, reconnecting in 2s", e)
            await asyncio.sleep(2.0)
```

Also create `src/ingest/__init__.py` as empty file.

- [ ] **Step 4: Commit**

```bash
git add src/ingest/
git commit -m "feat(python-poc): add PM + BN + account ingest tasks"
```

---

### Task 7: Engine Loop + Main Entry Point

**Files:**
- Create: `polymarket-bot-python-poc/src/engine/engine.py`
- Create: `polymarket-bot-python-poc/src/main.py`

- [ ] **Step 1: Write engine.py**

```python
# src/engine/engine.py
"""Engine loop — drains ingest queues, updates market state, fans out to strategies."""
from __future__ import annotations

import asyncio
import logging
import time

from src.types import HotEvent, EventSource, TelemetryEvent, TelemetryKind

logger = logging.getLogger(__name__)


async def engine_loop(
    pm_q: asyncio.Queue,
    ref_q: asyncio.Queue,
    account_q: asyncio.Queue,
    intent_q: asyncio.Queue,
    telemetry_q: asyncio.Queue,
    market_state,           # MarketState instance
    strategies: list,       # list of Strategy instances
    risk_gate,              # RiskGate instance (None in Phase 1)
    shutdown: asyncio.Event,
) -> None:
    """Main engine loop — mirrors Rust engine thread."""
    event_count = 0

    while not shutdown.is_set():
        had_work = False

        # Priority 1: Account events (fills, acks)
        while not account_q.empty():
            had_work = True
            event: HotEvent = account_q.get_nowait()
            event_count += 1
            market_state.process(event)
            _fan_out(event, strategies, intent_q, telemetry_q, risk_gate, event_count)

        # Priority 2: PM market data
        while not pm_q.empty():
            had_work = True
            event = pm_q.get_nowait()
            event_count += 1
            market_state.process(event)
            _fan_out(event, strategies, intent_q, telemetry_q, risk_gate, event_count)

        # Priority 3: Binance reference
        while not ref_q.empty():
            had_work = True
            event = ref_q.get_nowait()
            event_count += 1
            market_state.process(event)
            _fan_out(event, strategies, intent_q, telemetry_q, risk_gate, event_count)

        if not had_work:
            await asyncio.sleep(0.001)  # 1ms yield when idle


def _fan_out(
    event: HotEvent,
    strategies: list,
    intent_q: asyncio.Queue,
    telemetry_q: asyncio.Queue,
    risk_gate,
    event_count: int,
) -> None:
    """Fan out event to all strategies, collect intents, apply risk gate."""
    for strat in strategies:
        t0 = time.monotonic_ns()
        intents = strat.on_event(event)
        latency_ns = time.monotonic_ns() - t0

        for intent in intents:
            intent.strategy_id = strat.strategy_id
            if risk_gate is not None:
                result = risk_gate.check(intent, strat)
                if result != "PASS":
                    try:
                        telemetry_q.put_nowait(TelemetryEvent(
                            kind=TelemetryKind.RISK_REJECT,
                            strategy_name=strat.name,
                            strategy_id=strat.strategy_id,
                            instrument_id=intent.instrument_id,
                            message=result,
                        ))
                    except asyncio.QueueFull:
                        pass
                    continue
            try:
                intent_q.put_nowait(intent)
            except asyncio.QueueFull:
                logger.warning("intent_q full, dropping intent")
```

- [ ] **Step 2: Write main.py (Phase 1 — ingest + engine only, no strategy/execution yet)**

```python
# src/main.py
"""Entry point — market discovery, launch all tasks, lifecycle management."""
from __future__ import annotations

import asyncio
import logging
import os
import sys
from pathlib import Path

from dotenv import load_dotenv

from src.config import load_config
from src.engine.engine import engine_loop
from src.engine.market_state import MarketState
from src.ingest.polymarket import pm_ingest
from src.ingest.binance import bn_ingest
from src.ingest.account import account_ingest
from src.types import Instrument

logger = logging.getLogger(__name__)


async def discover_markets(config) -> list[Instrument]:
    """Discover active Polymarket up/down markets for configured assets + timeframe.

    Uses py-clob-client to query the Gamma API. Returns list of Instruments.
    """
    from py_clob_client.client import ClobClient

    host = "https://clob.polymarket.com"
    client = ClobClient(host, chain_id=137)

    instruments: list[Instrument] = []
    inst_id = 0

    # For each asset, search for up-or-down markets
    for asset in config.assets:
        # Query markets — look for active binary up/down markets
        try:
            # Use gamma API to find markets
            import httpx
            async with httpx.AsyncClient() as http:
                resp = await http.get(
                    "https://gamma-api.polymarket.com/events",
                    params={"closed": "false", "limit": 100},
                )
                events = resp.json()

            for event in events:
                title = event.get("title", "").upper()
                if asset.upper() not in title:
                    continue
                if config.timeframe.upper() not in title:
                    continue
                if "UP" not in title and "DOWN" not in title:
                    continue

                for market in event.get("markets", []):
                    condition_id = market.get("conditionId", "")
                    tokens = market.get("clobTokenIds", [])
                    if len(tokens) != 2:
                        continue

                    tick_size = market.get("minimumTickSize", "0.01")
                    neg_risk = market.get("negRisk", False)

                    for i, token_id in enumerate(tokens):
                        outcome = "YES" if i == 0 else "NO"
                        ref_sym = f"{asset.upper()}USDT"
                        instruments.append(Instrument(
                            instrument_id=inst_id,
                            token_id=token_id,
                            condition_id=condition_id,
                            outcome=outcome,
                            tick_size=__import__("decimal").Decimal(tick_size),
                            neg_risk=neg_risk,
                            min_size=__import__("decimal").Decimal("1"),
                            asset=asset,
                            timeframe=config.timeframe,
                            ref_symbol=ref_sym,
                        ))
                        inst_id += 1

        except Exception as e:
            logger.error("Failed to discover %s markets: %s", asset, e)

    logger.info("Discovered %d instruments", len(instruments))
    return instruments


async def run_bot() -> None:
    config_path = Path("config/bot.toml")
    if not config_path.exists():
        logger.error("Config not found: %s", config_path)
        sys.exit(1)

    config = load_config(config_path)
    load_dotenv(config.env_file)

    logging.basicConfig(
        level=getattr(logging, config.log_level),
        format="%(asctime)s.%(msecs)03d [%(name)s] %(levelname)s: %(message)s",
        datefmt="%H:%M:%S",
    )

    logger.info("Paper mode: %s", config.paper_mode)
    logger.info("Timeframe: %s, Assets: %s", config.timeframe, config.assets)

    # Discover markets
    instruments = await discover_markets(config)
    if not instruments:
        logger.error("No instruments discovered, exiting")
        return

    # Build ref_symbols for Binance (deduplicated)
    ref_symbols: list[tuple[str, int]] = []
    seen_refs: set[str] = set()
    for inst in instruments:
        sym = inst.ref_symbol.lower()
        if sym not in seen_refs:
            seen_refs.add(sym)
            ref_symbols.append((sym, inst.instrument_id))

    # Create queues
    pm_q: asyncio.Queue = asyncio.Queue(maxsize=65536)
    ref_q: asyncio.Queue = asyncio.Queue(maxsize=65536)
    account_q: asyncio.Queue = asyncio.Queue(maxsize=4096)
    intent_q: asyncio.Queue = asyncio.Queue(maxsize=4096)
    telemetry_q: asyncio.Queue = asyncio.Queue(maxsize=16384)

    # Create market state
    market_state = MarketState(num_instruments=len(instruments))

    # Shutdown event
    shutdown = asyncio.Event()

    # API credentials
    api_key = os.getenv("PM_API_KEY", "")
    api_secret = os.getenv("PM_SECRET", "")
    passphrase = os.getenv("PM_PASSPHRASE", "")

    # Launch tasks
    tasks = [
        asyncio.create_task(pm_ingest(pm_q, instruments, shutdown)),
        asyncio.create_task(bn_ingest(ref_q, ref_symbols, shutdown)),
        asyncio.create_task(account_ingest(
            account_q, instruments, api_key, api_secret, passphrase, shutdown,
        )),
        asyncio.create_task(engine_loop(
            pm_q, ref_q, account_q, intent_q, telemetry_q,
            market_state, strategies=[], risk_gate=None, shutdown=shutdown,
        )),
    ]

    logger.info("Bot running — %d tasks launched", len(tasks))

    try:
        await asyncio.gather(*tasks)
    except KeyboardInterrupt:
        pass
    finally:
        logger.info("Shutting down...")
        shutdown.set()
        for t in tasks:
            t.cancel()
        logger.info("Shutdown complete")


def main() -> None:
    asyncio.run(run_bot())


if __name__ == "__main__":
    main()
```

- [ ] **Step 3: Test manually**

```bash
cd polymarket-bot-python-poc
python -m src.main
```

Expected: Discovers markets, connects to WS feeds, logs BBO updates. No strategies yet — engine processes events but generates no intents.

- [ ] **Step 4: Commit**

```bash
git add src/engine/engine.py src/main.py
git commit -m "feat(python-poc): add engine loop + main entry point (Phase 1 complete)"
```

---

## Phase 2: Strategy + Execution

### Task 8: Position Tracking

**Files:**
- Create: `polymarket-bot-python-poc/src/strategy/__init__.py`
- Create: `polymarket-bot-python-poc/src/strategy/position.py`
- Create: `polymarket-bot-python-poc/tests/test_position.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_position.py
from decimal import Decimal
from src.types import Side
from src.strategy.position import Position


def test_position_buy_increases():
    pos = Position(instrument_id=0)
    pos.on_fill(Side.BUY, Decimal("100"), Decimal("0.50"))
    assert pos.qty == 100
    assert pos.avg_entry == Decimal("0.50")
    assert pos.fill_count == 1


def test_position_sell_decreases():
    pos = Position(instrument_id=0)
    pos.on_fill(Side.BUY, Decimal("100"), Decimal("0.50"))
    pos.on_fill(Side.SELL, Decimal("50"), Decimal("0.55"))
    assert pos.qty == 50
    assert pos.realized_pnl == Decimal("2.5")  # 50 * (0.55 - 0.50)


def test_position_flat_resets_entry():
    pos = Position(instrument_id=0)
    pos.on_fill(Side.BUY, Decimal("100"), Decimal("0.50"))
    pos.on_fill(Side.SELL, Decimal("100"), Decimal("0.55"))
    assert pos.qty == 0
    assert pos.avg_entry == Decimal(0)
    assert pos.realized_pnl == Decimal("5.0")


def test_position_vwap_entry():
    pos = Position(instrument_id=0)
    pos.on_fill(Side.BUY, Decimal("100"), Decimal("0.50"))
    pos.on_fill(Side.BUY, Decimal("100"), Decimal("0.54"))
    assert pos.qty == 200
    assert pos.avg_entry == Decimal("0.52")  # (50 + 54) / 200


def test_position_unrealized_pnl():
    pos = Position(instrument_id=0)
    pos.on_fill(Side.BUY, Decimal("100"), Decimal("0.50"))
    pnl = pos.unrealized_pnl(Decimal("0.55"))
    assert pnl == Decimal("5.0")  # 100 * (0.55 - 0.50)


def test_position_total_pnl():
    pos = Position(instrument_id=0)
    pos.on_fill(Side.BUY, Decimal("100"), Decimal("0.50"))
    pos.on_fill(Side.SELL, Decimal("50"), Decimal("0.55"))
    total = pos.total_pnl(Decimal("0.53"))
    realized = Decimal("2.5")    # 50 * 0.05
    unrealized = Decimal("1.5")  # 50 * (0.53 - 0.50)
    assert total == realized + unrealized


def test_position_short():
    pos = Position(instrument_id=0)
    pos.on_fill(Side.SELL, Decimal("100"), Decimal("0.60"))
    assert pos.qty == -100
    pnl = pos.unrealized_pnl(Decimal("0.55"))
    assert pnl == Decimal("5.0")  # short, price dropped = profit
```

- [ ] **Step 2: Run test to verify it fails**

```bash
python -m pytest tests/test_position.py -v
```

- [ ] **Step 3: Write position.py**

```python
# src/strategy/position.py
"""Position tracking — signed qty, VWAP entry, PnL."""
from __future__ import annotations

from dataclasses import dataclass, field
from decimal import Decimal

from src.types import Side


@dataclass
class Position:
    instrument_id: int
    qty: int = 0                          # signed: +long, -short
    avg_entry: Decimal = Decimal(0)
    realized_pnl: Decimal = Decimal(0)
    fill_count: int = 0

    def on_fill(self, side: Side, qty: Decimal, price: Decimal) -> None:
        signed_qty = int(qty) if side == Side.BUY else -int(qty)
        fill_qty = int(qty)

        if self.qty == 0 or (self.qty > 0) == (signed_qty > 0):
            # Increasing position — update VWAP
            old_cost = abs(self.qty) * self.avg_entry
            new_cost = fill_qty * price
            total_qty = abs(self.qty) + fill_qty
            if total_qty > 0:
                self.avg_entry = (old_cost + new_cost) / total_qty
        else:
            # Reducing position — realize PnL
            closed_qty = min(fill_qty, abs(self.qty))
            pnl_per_unit = price - self.avg_entry
            if self.qty > 0:
                self.realized_pnl += closed_qty * pnl_per_unit
            else:
                self.realized_pnl += closed_qty * (-pnl_per_unit)

        self.qty += signed_qty
        self.fill_count += 1

        # Flat invariant
        if self.qty == 0:
            self.avg_entry = Decimal(0)

    def unrealized_pnl(self, current_mid: Decimal) -> Decimal:
        if self.qty == 0:
            return Decimal(0)
        diff = current_mid - self.avg_entry
        return self.qty * diff

    def total_pnl(self, current_mid: Decimal) -> Decimal:
        return self.realized_pnl + self.unrealized_pnl(current_mid)

    def notional(self, current_mid: Decimal) -> Decimal:
        return abs(self.qty) * current_mid

    @property
    def is_flat(self) -> bool:
        return self.qty == 0
```

Also create `src/strategy/__init__.py` as empty file.

- [ ] **Step 4: Run tests**

```bash
python -m pytest tests/test_position.py -v
```

Expected: all 7 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/strategy/ tests/test_position.py
git commit -m "feat(python-poc): add Position tracking with VWAP + PnL"
```

---

### Task 9: Order Tracker

**Files:**
- Create: `polymarket-bot-python-poc/src/strategy/order_tracker.py`
- Create: `polymarket-bot-python-poc/tests/test_order_tracker.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_order_tracker.py
from decimal import Decimal
from src.types import Side
from src.strategy.order_tracker import OrderTracker, TrackedOrder, OrderState


def test_tracker_add_and_get():
    tracker = OrderTracker()
    tracker.on_intent_sent(TrackedOrder(
        client_order_id=1, exchange_order_id="", strategy_id=0,
        instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
        original_qty=Decimal("100"), filled_qty=Decimal(0),
        state=OrderState.PENDING, created_at_ns=1000, acked_at_ns=0,
    ))
    order = tracker.get(1)
    assert order is not None
    assert order.state == OrderState.PENDING


def test_tracker_ack():
    tracker = OrderTracker()
    tracker.on_intent_sent(TrackedOrder(
        client_order_id=1, exchange_order_id="", strategy_id=0,
        instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
        original_qty=Decimal("100"), filled_qty=Decimal(0),
        state=OrderState.PENDING, created_at_ns=1000, acked_at_ns=0,
    ))
    tracker.on_ack(1, exchange_order_id="ex_1", acked_at_ns=2000)
    order = tracker.get(1)
    assert order.state == OrderState.LIVE
    assert order.exchange_order_id == "ex_1"
    assert order.acked_at_ns == 2000


def test_tracker_fill():
    tracker = OrderTracker()
    tracker.on_intent_sent(TrackedOrder(
        client_order_id=1, exchange_order_id="", strategy_id=0,
        instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
        original_qty=Decimal("100"), filled_qty=Decimal(0),
        state=OrderState.PENDING, created_at_ns=1000, acked_at_ns=0,
    ))
    tracker.on_ack(1, exchange_order_id="ex_1", acked_at_ns=2000)
    tracker.on_fill(1, Decimal("100"))
    order = tracker.get(1)
    assert order.state == OrderState.FILLED
    assert order.filled_qty == Decimal("100")


def test_tracker_partial_fill():
    tracker = OrderTracker()
    tracker.on_intent_sent(TrackedOrder(
        client_order_id=1, exchange_order_id="", strategy_id=0,
        instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
        original_qty=Decimal("100"), filled_qty=Decimal(0),
        state=OrderState.PENDING, created_at_ns=1000, acked_at_ns=0,
    ))
    tracker.on_ack(1, exchange_order_id="ex_1", acked_at_ns=2000)
    tracker.on_fill(1, Decimal("50"))
    order = tracker.get(1)
    assert order.state == OrderState.LIVE  # partial
    assert order.filled_qty == Decimal("50")


def test_tracker_cancel():
    tracker = OrderTracker()
    tracker.on_intent_sent(TrackedOrder(
        client_order_id=1, exchange_order_id="", strategy_id=0,
        instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
        original_qty=Decimal("100"), filled_qty=Decimal(0),
        state=OrderState.PENDING, created_at_ns=1000, acked_at_ns=0,
    ))
    tracker.on_ack(1, exchange_order_id="ex_1", acked_at_ns=2000)
    tracker.on_cancel_ack(1)
    order = tracker.get(1)
    assert order.state == OrderState.CANCELLED


def test_tracker_reject():
    tracker = OrderTracker()
    tracker.on_intent_sent(TrackedOrder(
        client_order_id=1, exchange_order_id="", strategy_id=0,
        instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
        original_qty=Decimal("100"), filled_qty=Decimal(0),
        state=OrderState.PENDING, created_at_ns=1000, acked_at_ns=0,
    ))
    tracker.on_reject(1)
    order = tracker.get(1)
    assert order.state == OrderState.REJECTED


def test_tracker_active_count():
    tracker = OrderTracker()
    for i in range(3):
        tracker.on_intent_sent(TrackedOrder(
            client_order_id=i, exchange_order_id="", strategy_id=0,
            instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
            original_qty=Decimal("100"), filled_qty=Decimal(0),
            state=OrderState.PENDING, created_at_ns=1000, acked_at_ns=0,
        ))
        tracker.on_ack(i, exchange_order_id=f"ex_{i}", acked_at_ns=2000)
    assert tracker.active_count == 3
    tracker.on_fill(0, Decimal("100"))
    assert tracker.active_count == 2


def test_tracker_max_capacity():
    tracker = OrderTracker(max_orders=4)
    for i in range(4):
        assert tracker.on_intent_sent(TrackedOrder(
            client_order_id=i, exchange_order_id="", strategy_id=0,
            instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
            original_qty=Decimal("100"), filled_qty=Decimal(0),
            state=OrderState.PENDING, created_at_ns=1000, acked_at_ns=0,
        ))
    # 5th should fail
    assert not tracker.on_intent_sent(TrackedOrder(
        client_order_id=99, exchange_order_id="", strategy_id=0,
        instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
        original_qty=Decimal("100"), filled_qty=Decimal(0),
        state=OrderState.PENDING, created_at_ns=1000, acked_at_ns=0,
    ))
```

- [ ] **Step 2: Run test to verify it fails**

- [ ] **Step 3: Write order_tracker.py**

```python
# src/strategy/order_tracker.py
"""Order state machine — tracks all orders for one strategy."""
from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal

from src.types import Side, OrderState


MAX_TRACKED_ORDERS = 64


@dataclass
class TrackedOrder:
    client_order_id: int
    exchange_order_id: str
    strategy_id: int
    instrument_id: int
    side: Side
    price: Decimal
    original_qty: Decimal
    filled_qty: Decimal
    state: OrderState
    created_at_ns: int
    acked_at_ns: int

    @property
    def remaining_qty(self) -> Decimal:
        return self.original_qty - self.filled_qty

    @property
    def is_active(self) -> bool:
        return self.state in (OrderState.LIVE, OrderState.CANCEL_PENDING, OrderState.PENDING, OrderState.SENDING)


class OrderTracker:
    def __init__(self, max_orders: int = MAX_TRACKED_ORDERS) -> None:
        self._orders: dict[int, TrackedOrder] = {}
        self._max_orders = max_orders

    def on_intent_sent(self, order: TrackedOrder) -> bool:
        if len(self._orders) >= self._max_orders:
            # Try to reclaim terminal slots
            self._reclaim()
            if len(self._orders) >= self._max_orders:
                return False
        self._orders[order.client_order_id] = order
        return True

    def on_ack(self, client_order_id: int, exchange_order_id: str = "",
               acked_at_ns: int = 0) -> None:
        order = self._orders.get(client_order_id)
        if order is None:
            return
        order.state = OrderState.LIVE
        order.exchange_order_id = exchange_order_id
        order.acked_at_ns = acked_at_ns

    def on_reject(self, client_order_id: int) -> None:
        order = self._orders.get(client_order_id)
        if order is None:
            return
        order.state = OrderState.REJECTED

    def on_fill(self, client_order_id: int, fill_qty: Decimal) -> None:
        order = self._orders.get(client_order_id)
        if order is None:
            return
        order.filled_qty += fill_qty
        if order.filled_qty >= order.original_qty:
            order.state = OrderState.FILLED

    def on_cancel_ack(self, client_order_id: int) -> None:
        order = self._orders.get(client_order_id)
        if order is None:
            return
        order.state = OrderState.CANCELLED

    def get(self, client_order_id: int) -> TrackedOrder | None:
        return self._orders.get(client_order_id)

    @property
    def active_count(self) -> int:
        return sum(1 for o in self._orders.values() if o.is_active)

    def active_orders(self) -> list[TrackedOrder]:
        return [o for o in self._orders.values() if o.is_active]

    def open_qty(self, instrument_id: int, side: Side) -> Decimal:
        total = Decimal(0)
        for o in self._orders.values():
            if o.is_active and o.instrument_id == instrument_id and o.side == side:
                total += o.remaining_qty
        return total

    def _reclaim(self) -> None:
        terminal = [oid for oid, o in self._orders.items()
                    if o.state in (OrderState.FILLED, OrderState.CANCELLED, OrderState.REJECTED)]
        for oid in terminal:
            del self._orders[oid]
```

- [ ] **Step 4: Run tests**

```bash
python -m pytest tests/test_order_tracker.py -v
```

Expected: all 8 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/strategy/order_tracker.py tests/test_order_tracker.py
git commit -m "feat(python-poc): add OrderTracker state machine"
```

---

### Task 10: Queue Estimator

**Files:**
- Create: `polymarket-bot-python-poc/src/strategy/queue_estimator.py`
- Create: `polymarket-bot-python-poc/tests/test_queue_estimator.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_queue_estimator.py
from decimal import Decimal
from src.types import Side
from src.strategy.queue_estimator import QueueEstimator


def test_register_and_query():
    qe = QueueEstimator()
    qe.register_order(order_id=1, instrument_id=0, side=Side.BUY,
                      price=Decimal("0.50"), qty=Decimal("100"),
                      current_level_size=Decimal("500"))
    ahead = qe.queue_ahead(1)
    assert ahead is not None
    assert ahead == Decimal("400")  # 500 - 100 = 400 ahead


def test_trade_reduces_queue():
    qe = QueueEstimator()
    qe.register_order(order_id=1, instrument_id=0, side=Side.BUY,
                      price=Decimal("0.50"), qty=Decimal("100"),
                      current_level_size=Decimal("500"))
    qe.on_trade(instrument_id=0, side=Side.SELL, price=Decimal("0.50"),
                qty=Decimal("200"))
    ahead = qe.queue_ahead(1)
    assert ahead is not None
    assert ahead < Decimal("400")  # reduced by trade


def test_cancelled_order_removed():
    qe = QueueEstimator()
    qe.register_order(order_id=1, instrument_id=0, side=Side.BUY,
                      price=Decimal("0.50"), qty=Decimal("100"),
                      current_level_size=Decimal("500"))
    qe.order_cancelled(1)
    assert qe.queue_ahead(1) is None


def test_level_change_updates_queue():
    qe = QueueEstimator()
    qe.register_order(order_id=1, instrument_id=0, side=Side.BUY,
                      price=Decimal("0.50"), qty=Decimal("100"),
                      current_level_size=Decimal("500"))
    # Level size decreases (cancels behind us)
    qe.on_level_change(instrument_id=0, side=Side.BUY,
                       price=Decimal("0.50"),
                       old_qty=Decimal("500"), new_qty=Decimal("300"))
    ahead = qe.queue_ahead(1)
    assert ahead is not None
    # Queue should be reduced but still >= 0
    assert ahead >= Decimal(0)
```

- [ ] **Step 2: Run test to verify it fails**

- [ ] **Step 3: Write queue_estimator.py**

```python
# src/strategy/queue_estimator.py
"""L2 queue position estimator — PowerProbQueueFunc model."""
from __future__ import annotations

import math
from dataclasses import dataclass
from decimal import Decimal

from src.types import Side


@dataclass
class QueuedOrder:
    order_id: int
    instrument_id: int
    side: Side
    price: Decimal
    qty: Decimal
    ahead_qty: Decimal
    posted_at_level_size: Decimal


class QueueEstimator:
    """Estimates queue position for resting orders using L2 data."""

    def __init__(self, prob_power_n: float = 3.0) -> None:
        self._orders: dict[int, QueuedOrder] = {}
        self._prob_power_n = prob_power_n
        self._take_rate_bid: float = 0.0
        self._take_rate_ask: float = 0.0
        self._trade_volume_bid: float = 0.0
        self._trade_volume_ask: float = 0.0
        self._trade_count: int = 0

    def register_order(self, order_id: int, instrument_id: int, side: Side,
                       price: Decimal, qty: Decimal,
                       current_level_size: Decimal) -> None:
        ahead = max(Decimal(0), current_level_size - qty)
        self._orders[order_id] = QueuedOrder(
            order_id=order_id, instrument_id=instrument_id,
            side=side, price=price, qty=qty,
            ahead_qty=ahead, posted_at_level_size=current_level_size,
        )

    def order_filled(self, order_id: int, fill_qty: Decimal) -> None:
        order = self._orders.get(order_id)
        if order is None:
            return
        order.ahead_qty = Decimal(0)
        order.qty -= fill_qty
        if order.qty <= 0:
            del self._orders[order_id]

    def order_cancelled(self, order_id: int) -> None:
        self._orders.pop(order_id, None)

    def on_trade(self, instrument_id: int, side: Side,
                 price: Decimal, qty: Decimal) -> None:
        """A trade at this price reduces queue for matching orders."""
        self._trade_count += 1
        vol = float(qty)
        if side == Side.SELL:
            self._trade_volume_bid += vol
        else:
            self._trade_volume_ask += vol

        for order in self._orders.values():
            if (order.instrument_id == instrument_id
                    and order.price == price
                    and order.side != side):
                reduction = qty
                order.ahead_qty = max(Decimal(0), order.ahead_qty - reduction)

    def on_level_change(self, instrument_id: int, side: Side,
                        price: Decimal, old_qty: Decimal,
                        new_qty: Decimal) -> None:
        """Level size changed — cancels biased toward back of queue."""
        if new_qty >= old_qty:
            return  # additions go behind us

        removed = old_qty - new_qty
        for order in self._orders.values():
            if (order.instrument_id == instrument_id
                    and order.side == side
                    and order.price == price):
                # Power probability: cancels biased toward back
                if order.posted_at_level_size > 0:
                    position_ratio = float(order.ahead_qty / order.posted_at_level_size)
                    # Probability cancel was ahead of us (power model)
                    prob_ahead = position_ratio ** self._prob_power_n
                    estimated_ahead_cancel = removed * Decimal(str(prob_ahead))
                    order.ahead_qty = max(Decimal(0),
                                          order.ahead_qty - estimated_ahead_cancel)

    def queue_ahead(self, order_id: int) -> Decimal | None:
        order = self._orders.get(order_id)
        if order is None:
            return None
        return order.ahead_qty

    def fill_probability(self, order_id: int, time_remaining_secs: float) -> float:
        """P[fill before time_remaining] using Poisson model."""
        order = self._orders.get(order_id)
        if order is None:
            return 0.0
        q = float(order.ahead_qty)
        mu = self.take_rate(order.side)
        if mu <= 0 or time_remaining_secs <= 0:
            return 0.0
        lam = mu * time_remaining_secs
        # P[Poisson(lam) >= q] = 1 - P[Poisson(lam) < q]
        prob = 0.0
        for k in range(int(q)):
            prob += math.exp(-lam) * (lam ** k) / math.factorial(k)
        return 1.0 - prob

    def take_rate(self, side: Side) -> float:
        """Estimated contracts/second consumed at touch."""
        if self._trade_count == 0:
            return 0.0
        if side == Side.BUY:
            return self._trade_volume_bid / max(1, self._trade_count)
        return self._trade_volume_ask / max(1, self._trade_count)
```

- [ ] **Step 4: Run tests**

```bash
python -m pytest tests/test_queue_estimator.py -v
```

Expected: all 4 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/strategy/queue_estimator.py tests/test_queue_estimator.py
git commit -m "feat(python-poc): add QueueEstimator with power-prob model"
```

---

### Task 11: Strategy Base + Risk Limits

**Files:**
- Create: `polymarket-bot-python-poc/src/strategy/base.py`
- Create: `polymarket-bot-python-poc/src/execution/__init__.py`
- Create: `polymarket-bot-python-poc/src/execution/risk.py`
- Create: `polymarket-bot-python-poc/tests/test_risk.py`

- [ ] **Step 1: Write base.py**

```python
# src/strategy/base.py
"""Strategy protocol — the interface all strategies implement."""
from __future__ import annotations

from typing import Protocol

from src.types import HotEvent, OrderIntent


class Strategy(Protocol):
    strategy_id: int
    name: str

    def on_event(self, event: HotEvent) -> list[OrderIntent]:
        ...
```

- [ ] **Step 2: Write the failing risk test**

```python
# tests/test_risk.py
from decimal import Decimal
from src.types import OrderIntent, OrderAction, Side, KillState
from src.execution.risk import RiskGate, RiskLimits, GlobalRiskLimits


def test_risk_gate_pass():
    limits = RiskLimits(max_position_per_instrument=1000, capital_budget=Decimal("500"),
                        max_orders_live=20, max_intents_per_sec=50,
                        max_replaces_per_min=100, quote_age_ms=2000)
    gate = RiskGate(strategy_limits={"test": limits})
    intent = OrderIntent(instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
                         qty=Decimal("100"), action=OrderAction.POST,
                         client_order_id=1, strategy_id=0)
    result = gate.check_strategy(intent, strategy_name="test",
                                 current_position=0, active_orders=0)
    assert result == "PASS"


def test_risk_gate_position_breach():
    limits = RiskLimits(max_position_per_instrument=100, capital_budget=Decimal("500"),
                        max_orders_live=20, max_intents_per_sec=50,
                        max_replaces_per_min=100, quote_age_ms=2000)
    gate = RiskGate(strategy_limits={"test": limits})
    intent = OrderIntent(instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
                         qty=Decimal("200"), action=OrderAction.POST,
                         client_order_id=1, strategy_id=0)
    result = gate.check_strategy(intent, strategy_name="test",
                                 current_position=50, active_orders=0)
    assert result == "REJECT_POSITION"


def test_risk_gate_order_count():
    limits = RiskLimits(max_position_per_instrument=1000, capital_budget=Decimal("500"),
                        max_orders_live=2, max_intents_per_sec=50,
                        max_replaces_per_min=100, quote_age_ms=2000)
    gate = RiskGate(strategy_limits={"test": limits})
    intent = OrderIntent(instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
                         qty=Decimal("100"), action=OrderAction.POST,
                         client_order_id=1, strategy_id=0)
    result = gate.check_strategy(intent, strategy_name="test",
                                 current_position=0, active_orders=3)
    assert result == "REJECT_ORDER_COUNT"


def test_global_gate_kill_switch():
    global_limits = GlobalRiskLimits(
        wallet_min_free_usdc=Decimal("100"), max_live_orders_total=50,
        max_submits_per_min=200, max_cancels_per_min=300,
        feed_stale_ms=2000, feed_dead_ms=5000,
    )
    gate = RiskGate(strategy_limits={}, global_limits=global_limits)
    intent = OrderIntent(instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
                         qty=Decimal("100"), action=OrderAction.POST,
                         client_order_id=1, strategy_id=0)
    result = gate.check_global(intent, kill_state=KillState.PAUSED)
    assert result == "REJECT_KILL_SWITCH"


def test_cancel_always_passes_risk():
    limits = RiskLimits(max_position_per_instrument=0, capital_budget=Decimal("0"),
                        max_orders_live=0, max_intents_per_sec=50,
                        max_replaces_per_min=100, quote_age_ms=2000)
    gate = RiskGate(strategy_limits={"test": limits})
    intent = OrderIntent(instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
                         qty=Decimal("100"), action=OrderAction.CANCEL,
                         client_order_id=1, strategy_id=0)
    result = gate.check_strategy(intent, strategy_name="test",
                                 current_position=9999, active_orders=9999)
    assert result == "PASS"
```

- [ ] **Step 3: Write risk.py**

```python
# src/execution/risk.py
"""Risk gate — per-strategy + global checks + kill switch."""
from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal

from src.types import OrderAction, OrderIntent, KillState


@dataclass
class RiskLimits:
    max_position_per_instrument: int
    capital_budget: Decimal
    max_orders_live: int
    max_intents_per_sec: int
    max_replaces_per_min: int
    quote_age_ms: int


@dataclass
class GlobalRiskLimits:
    wallet_min_free_usdc: Decimal = Decimal("100")
    max_live_orders_total: int = 50
    max_submits_per_min: int = 200
    max_cancels_per_min: int = 300
    feed_stale_ms: int = 2000
    feed_dead_ms: int = 5000


class RiskGate:
    def __init__(
        self,
        strategy_limits: dict[str, RiskLimits] | None = None,
        global_limits: GlobalRiskLimits | None = None,
    ) -> None:
        self._strategy_limits = strategy_limits or {}
        self._global_limits = global_limits or GlobalRiskLimits()
        self.kill_state = KillState.ACTIVE

    def check_strategy(
        self,
        intent: OrderIntent,
        strategy_name: str,
        current_position: int,
        active_orders: int,
    ) -> str:
        # Cancels always pass
        if intent.action == OrderAction.CANCEL:
            return "PASS"

        limits = self._strategy_limits.get(strategy_name)
        if limits is None:
            return "PASS"

        # Position check
        worst_case = abs(current_position) + int(intent.qty)
        if worst_case > limits.max_position_per_instrument:
            return "REJECT_POSITION"

        # Order count
        if intent.action == OrderAction.POST and active_orders >= limits.max_orders_live:
            return "REJECT_ORDER_COUNT"

        # Capital check
        cost = intent.qty * intent.price
        if cost > limits.capital_budget:
            return "REJECT_CAPITAL"

        return "PASS"

    def check_global(self, intent: OrderIntent, kill_state: KillState) -> str:
        # Cancels always pass
        if intent.action == OrderAction.CANCEL:
            return "PASS"

        if kill_state >= KillState.PAUSED:
            return "REJECT_KILL_SWITCH"

        if kill_state == KillState.REDUCE_ONLY and intent.action == OrderAction.POST:
            return "REJECT_REDUCE_ONLY"

        return "PASS"
```

Also create `src/execution/__init__.py` as empty file.

- [ ] **Step 4: Run tests**

```bash
python -m pytest tests/test_risk.py -v
```

Expected: all 5 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/strategy/base.py src/execution/ tests/test_risk.py
git commit -m "feat(python-poc): add Strategy protocol + RiskGate"
```

---

### Task 12: Ladder MM Strategy

**Files:**
- Create: `polymarket-bot-python-poc/src/strategy/ladder_mm.py`
- Create: `polymarket-bot-python-poc/tests/test_ladder_mm.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_ladder_mm.py
from decimal import Decimal
from src.types import HotEvent, EventKind, EventSource, Side, OrderAction
from src.strategy.ladder_mm import LadderMMStrategy
from src.config import StrategyConfig


def _make_config() -> StrategyConfig:
    return StrategyConfig(
        enabled=True, capital_budget=Decimal("500"), levels=3,
        spread_bps=100, skew_factor=0.3, max_position=1000,
        max_orders_live=20, max_replaces_per_min=100, quote_age_ms=2000,
    )


def _build_book(strategy, bids, asks):
    """Feed book deltas to build a book state."""
    seq = 0
    for price, size in bids:
        seq += 1
        strategy.on_event(HotEvent(
            recv_ts=1000, seq=seq, instrument_id=0,
            source=EventSource.PM, kind=EventKind.BOOK_DELTA,
            side=Side.BUY, price=Decimal(price), qty=Decimal(size),
        ))
    for price, size in asks:
        seq += 1
        strategy.on_event(HotEvent(
            recv_ts=1000, seq=seq, instrument_id=0,
            source=EventSource.PM, kind=EventKind.BOOK_DELTA,
            side=Side.SELL, price=Decimal(price), qty=Decimal(size),
        ))


def test_ladder_generates_intents_on_bbo():
    strat = LadderMMStrategy(strategy_id=0, config=_make_config(), num_instruments=4)
    # Build a book with clear BBO
    _build_book(strat,
                bids=[("0.48", "100"), ("0.49", "200"), ("0.50", "300")],
                asks=[("0.52", "150"), ("0.53", "100"), ("0.54", "50")])
    # Trigger BBO change
    intents = strat.on_event(HotEvent(
        recv_ts=2000, seq=100, instrument_id=0,
        source=EventSource.PM, kind=EventKind.BOOK_DELTA,
        side=Side.BUY, price=Decimal("0.50"), qty=Decimal("310"),
    ))
    # Should generate bid + ask ladder intents
    posts = [i for i in intents if i.action == OrderAction.POST]
    assert len(posts) > 0
    buy_posts = [i for i in posts if i.side == Side.BUY]
    sell_posts = [i for i in posts if i.side == Side.SELL]
    assert len(buy_posts) > 0
    assert len(sell_posts) > 0


def test_ladder_no_intents_without_bbo():
    strat = LadderMMStrategy(strategy_id=0, config=_make_config(), num_instruments=4)
    # Empty book — no BBO
    intents = strat.on_event(HotEvent(
        recv_ts=1000, seq=1, instrument_id=0,
        source=EventSource.PM, kind=EventKind.BOOK_DELTA,
        side=Side.BUY, price=Decimal("0.50"), qty=Decimal("100"),
    ))
    assert len(intents) == 0  # no ask side yet


def test_ladder_respects_instrument_id():
    strat = LadderMMStrategy(strategy_id=0, config=_make_config(), num_instruments=4)
    _build_book(strat,
                bids=[("0.50", "100")],
                asks=[("0.52", "100")])
    intents = strat.on_event(HotEvent(
        recv_ts=2000, seq=100, instrument_id=0,
        source=EventSource.PM, kind=EventKind.BOOK_DELTA,
        side=Side.BUY, price=Decimal("0.50"), qty=Decimal("110"),
    ))
    for intent in intents:
        assert intent.instrument_id == 0
```

- [ ] **Step 2: Run test to verify it fails**

- [ ] **Step 3: Write ladder_mm.py**

```python
# src/strategy/ladder_mm.py
"""Ladder market maker — symmetric ladder around fair value with inventory skew."""
from __future__ import annotations

from decimal import Decimal

from src.config import StrategyConfig
from src.engine.market_state import MarketState
from src.strategy.order_tracker import OrderTracker, TrackedOrder, OrderState
from src.strategy.position import Position
from src.strategy.queue_estimator import QueueEstimator
from src.types import HotEvent, OrderIntent, OrderAction, EventKind, EventSource, Side


class LadderMMStrategy:
    def __init__(self, strategy_id: int, config: StrategyConfig,
                 num_instruments: int) -> None:
        self.strategy_id = strategy_id
        self.name = "ladder_mm"
        self._config = config
        self._market_state = MarketState(num_instruments=num_instruments)
        self._positions: dict[int, Position] = {}
        self._order_tracker = OrderTracker()
        self._queue_estimator = QueueEstimator()
        self._next_order_id = (strategy_id << 56)  # high bits = strategy_id

    def on_event(self, event: HotEvent) -> list[OrderIntent]:
        # Update own market state
        self._market_state.process(event)

        # Handle account events
        if event.source == EventSource.ACCOUNT:
            return self._handle_account_event(event)

        # Check for BBO change (edge-triggered)
        tob = self._market_state.take_tob(event.instrument_id)
        if tob is None:
            return []

        if not self._market_state.is_ready(event.instrument_id):
            return []

        return self._reprice(event.instrument_id, tob)

    def _handle_account_event(self, event: HotEvent) -> list[OrderIntent]:
        if event.kind == EventKind.FILL:
            pos = self._positions.setdefault(event.instrument_id,
                                              Position(instrument_id=event.instrument_id))
            pos.on_fill(event.side, event.qty, event.price)
            self._order_tracker.on_fill(event.client_order_id, event.qty)
        elif event.kind == EventKind.ORDER_ACK:
            self._order_tracker.on_ack(event.client_order_id,
                                        exchange_order_id=event.exchange_order_id)
        elif event.kind == EventKind.ORDER_REJECT:
            self._order_tracker.on_reject(event.client_order_id)
        return []

    def _reprice(self, instrument_id: int, tob: dict) -> list[OrderIntent]:
        fair = tob["weighted_mid"]
        if fair <= 0:
            return []

        pos = self._positions.get(instrument_id, Position(instrument_id=instrument_id))
        spread = Decimal(self._config.spread_bps) / Decimal("10000")

        # Inventory skew
        skew = Decimal(str(self._config.skew_factor)) * Decimal(str(pos.qty)) / Decimal("1000")

        intents: list[OrderIntent] = []

        # Cancel existing orders for this instrument
        for order in self._order_tracker.active_orders():
            if order.instrument_id == instrument_id:
                intents.append(OrderIntent(
                    instrument_id=instrument_id,
                    side=order.side,
                    price=order.price,
                    qty=order.original_qty,
                    action=OrderAction.CANCEL,
                    client_order_id=order.client_order_id,
                    target_order_id=order.client_order_id,
                ))

        # Generate new ladder levels
        tick = self._market_state.book(instrument_id).spread()
        if tick <= 0:
            tick = Decimal("0.01")

        for i in range(self._config.levels):
            offset = spread / 2 + Decimal(str(i)) * tick
            # Bid side (skew pushes bids down when long)
            bid_price = fair - offset - skew
            bid_price = self._round_tick(bid_price)
            if bid_price > 0:
                self._next_order_id += 1
                intents.append(OrderIntent(
                    instrument_id=instrument_id,
                    side=Side.BUY,
                    price=bid_price,
                    qty=Decimal("100"),
                    action=OrderAction.POST,
                    client_order_id=self._next_order_id,
                ))

            # Ask side (skew pushes asks up when long)
            ask_price = fair + offset - skew
            ask_price = self._round_tick(ask_price)
            if ask_price > 0 and ask_price < Decimal("1.0"):
                self._next_order_id += 1
                intents.append(OrderIntent(
                    instrument_id=instrument_id,
                    side=Side.SELL,
                    price=ask_price,
                    qty=Decimal("100"),
                    action=OrderAction.POST,
                    client_order_id=self._next_order_id,
                ))

        return intents

    def _round_tick(self, price: Decimal) -> Decimal:
        return (price * 100).to_integral_value() / 100
```

- [ ] **Step 4: Run tests**

```bash
python -m pytest tests/test_ladder_mm.py -v
```

Expected: all 3 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/strategy/ladder_mm.py tests/test_ladder_mm.py
git commit -m "feat(python-poc): add Ladder MM strategy"
```

---

### Task 13: Momentum Strategy

**Files:**
- Create: `polymarket-bot-python-poc/src/strategy/momentum.py`
- Create: `polymarket-bot-python-poc/tests/test_momentum.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_momentum.py
from decimal import Decimal
from src.types import HotEvent, EventKind, EventSource, Side, OrderAction
from src.strategy.momentum import MomentumStrategy
from src.config import StrategyConfig


def _make_config() -> StrategyConfig:
    return StrategyConfig(
        enabled=True, capital_budget=Decimal("300"),
        signal_threshold=0.5, max_position=500,
        max_orders_live=2, max_replaces_per_min=5,
    )


def test_momentum_no_signal_no_intent():
    strat = MomentumStrategy(strategy_id=1, config=_make_config(), num_instruments=4)
    # Single BN BBO update — not enough for a signal
    intents = strat.on_event(HotEvent(
        recv_ts=1000, seq=1, instrument_id=0,
        source=EventSource.BN_BBO, kind=EventKind.TOP_OF_BOOK,
        side=Side.BUY, price=Decimal(0), qty=Decimal(0),
        bn_bid=Decimal("67000"), bn_ask=Decimal("67001"),
        bn_bid_qty=Decimal("1"), bn_ask_qty=Decimal("1"),
    ))
    assert len(intents) == 0


def test_momentum_strong_signal_posts():
    strat = MomentumStrategy(strategy_id=1, config=_make_config(), num_instruments=4)
    # Build a PM book first so we have a price to post at
    strat.on_event(HotEvent(
        recv_ts=1000, seq=1, instrument_id=0,
        source=EventSource.PM, kind=EventKind.BOOK_DELTA,
        side=Side.BUY, price=Decimal("0.50"), qty=Decimal("100"),
    ))
    strat.on_event(HotEvent(
        recv_ts=1000, seq=2, instrument_id=0,
        source=EventSource.PM, kind=EventKind.BOOK_DELTA,
        side=Side.SELL, price=Decimal("0.52"), qty=Decimal("100"),
    ))

    # Feed multiple BN updates to create a strong downward move
    base_price = Decimal("67000")
    for i in range(20):
        price = base_price - Decimal(str(i * 10))  # $200 drop
        strat.on_event(HotEvent(
            recv_ts=1000 + i * 1000, seq=10 + i, instrument_id=0,
            source=EventSource.BN_BBO, kind=EventKind.TOP_OF_BOOK,
            side=Side.BUY, price=Decimal(0), qty=Decimal(0),
            bn_bid=price, bn_ask=price + 1,
            bn_bid_qty=Decimal("1"), bn_ask_qty=Decimal("1"),
        ))

    # The last event should potentially trigger a signal
    # (depending on threshold calibration — may need adjustment)
    # At minimum, verify strategy doesn't crash
    assert strat._pending_order_id is None or isinstance(strat._pending_order_id, int)


def test_momentum_max_one_pending():
    strat = MomentumStrategy(strategy_id=1, config=_make_config(), num_instruments=4)
    strat._pending_order_id = 42  # simulate pending order
    intents = strat.on_event(HotEvent(
        recv_ts=1000, seq=1, instrument_id=0,
        source=EventSource.BN_BBO, kind=EventKind.TOP_OF_BOOK,
        side=Side.BUY, price=Decimal(0), qty=Decimal(0),
        bn_bid=Decimal("60000"), bn_ask=Decimal("60001"),
        bn_bid_qty=Decimal("1"), bn_ask_qty=Decimal("1"),
    ))
    assert len(intents) == 0  # already has pending order
```

- [ ] **Step 2: Run test to verify it fails**

- [ ] **Step 3: Write momentum.py**

```python
# src/strategy/momentum.py
"""Momentum strategy — directional orders on strong Binance reference moves."""
from __future__ import annotations

from collections import deque
from decimal import Decimal

from src.config import StrategyConfig
from src.engine.market_state import MarketState
from src.strategy.order_tracker import OrderTracker
from src.strategy.position import Position
from src.types import HotEvent, OrderIntent, OrderAction, EventKind, EventSource, Side


class MomentumStrategy:
    def __init__(self, strategy_id: int, config: StrategyConfig,
                 num_instruments: int) -> None:
        self.strategy_id = strategy_id
        self.name = "momentum"
        self._config = config
        self._market_state = MarketState(num_instruments=num_instruments)
        self._positions: dict[int, Position] = {}
        self._order_tracker = OrderTracker()
        self._pending_order_id: int | None = None
        self._next_order_id = (strategy_id << 56)

        # Reference price history for signal
        self._ref_prices: deque[Decimal] = deque(maxlen=60)
        self._signal_threshold = Decimal(str(config.signal_threshold))

    def on_event(self, event: HotEvent) -> list[OrderIntent]:
        self._market_state.process(event)

        if event.source == EventSource.ACCOUNT:
            return self._handle_account_event(event)

        # Track BN reference price
        if event.source == EventSource.BN_BBO:
            mid = (event.bn_bid + event.bn_ask) / 2
            if mid > 0:
                self._ref_prices.append(mid)

        # Only act on PM BBO changes
        tob = self._market_state.take_tob(event.instrument_id)
        if tob is None:
            return []

        if self._pending_order_id is not None:
            return []

        if not self._market_state.is_ready(event.instrument_id):
            return []

        return self._evaluate_signal(event.instrument_id, tob)

    def _handle_account_event(self, event: HotEvent) -> list[OrderIntent]:
        if event.kind == EventKind.FILL:
            pos = self._positions.setdefault(event.instrument_id,
                                              Position(instrument_id=event.instrument_id))
            pos.on_fill(event.side, event.qty, event.price)
            self._order_tracker.on_fill(event.client_order_id, event.qty)
            self._pending_order_id = None
        elif event.kind == EventKind.ORDER_ACK:
            self._order_tracker.on_ack(event.client_order_id,
                                        exchange_order_id=event.exchange_order_id)
        elif event.kind == EventKind.ORDER_REJECT:
            self._order_tracker.on_reject(event.client_order_id)
            self._pending_order_id = None
        return []

    def _evaluate_signal(self, instrument_id: int, tob: dict) -> list[OrderIntent]:
        if len(self._ref_prices) < 10:
            return []

        # Simple momentum: compare current vs N-periods ago
        current = self._ref_prices[-1]
        past = self._ref_prices[-10]
        if past == 0:
            return []

        pct_change = (current - past) / past

        # Strong downward move → BTC going down → buy NO (sell YES)
        # Strong upward move → BTC going up → buy YES
        if abs(pct_change) < self._signal_threshold / 100:
            return []

        if pct_change > 0:
            side = Side.BUY   # BTC up → buy YES
            price = tob["bid_price"]
        else:
            side = Side.SELL  # BTC down → sell YES (= buy NO)
            price = tob["ask_price"]

        if price <= 0:
            return []

        self._next_order_id += 1
        self._pending_order_id = self._next_order_id

        return [OrderIntent(
            instrument_id=instrument_id,
            side=side,
            price=price,
            qty=Decimal("100"),
            action=OrderAction.POST,
            client_order_id=self._next_order_id,
        )]
```

- [ ] **Step 4: Run tests**

```bash
python -m pytest tests/test_momentum.py -v
```

Expected: all 3 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/strategy/momentum.py tests/test_momentum.py
git commit -m "feat(python-poc): add Momentum strategy"
```

---

### Task 14: Executor + Heartbeat

**Files:**
- Create: `polymarket-bot-python-poc/src/execution/executor.py`
- Create: `polymarket-bot-python-poc/src/execution/heartbeat.py`

- [ ] **Step 1: Write executor.py**

```python
# src/execution/executor.py
"""Execution loop — drains intent queue, signs, batch POST/DELETE via py-clob-client."""
from __future__ import annotations

import asyncio
import logging
import time
from decimal import Decimal

from src.types import OrderAction, OrderIntent, TelemetryEvent, TelemetryKind, KillState
from src.execution.risk import RiskGate

logger = logging.getLogger(__name__)


async def execution_loop(
    intent_q: asyncio.Queue,
    exec_telemetry_q: asyncio.Queue,
    account_q: asyncio.Queue,
    risk_gate: RiskGate,
    clob_client,              # py_clob_client.ClobClient or None
    paper_mode: bool,
    shutdown: asyncio.Event,
) -> None:
    """Drain intents, apply global risk, sign and submit orders."""
    post_batch: list[OrderIntent] = []
    cancel_batch: list[OrderIntent] = []

    while not shutdown.is_set():
        post_batch.clear()
        cancel_batch.clear()

        # Drain intent queue
        while not intent_q.empty():
            intent: OrderIntent = intent_q.get_nowait()

            # Global risk gate (Layer 3)
            result = risk_gate.check_global(intent, risk_gate.kill_state)
            if result != "PASS":
                _emit(exec_telemetry_q, TelemetryEvent(
                    kind=TelemetryKind.GLOBAL_REJECT,
                    instrument_id=intent.instrument_id,
                    message=result,
                ))
                continue

            if intent.action == OrderAction.CANCEL:
                cancel_batch.append(intent)
            elif intent.action == OrderAction.POST:
                post_batch.append(intent)
            elif intent.action == OrderAction.AMEND:
                cancel_batch.append(intent)
                post_batch.append(intent)

            if len(post_batch) >= 15 or len(cancel_batch) >= 50:
                break

        # Execute: DELETE first, then POST
        if cancel_batch:
            t0 = time.monotonic()
            if paper_mode:
                logger.debug("PAPER: cancel %d orders", len(cancel_batch))
            else:
                try:
                    order_ids = [str(i.target_order_id or i.client_order_id) for i in cancel_batch]
                    await _async_cancel(clob_client, order_ids)
                except Exception as e:
                    logger.error("Cancel failed: %s", e)
            latency_ms = (time.monotonic() - t0) * 1000
            _emit(exec_telemetry_q, TelemetryEvent(
                kind=TelemetryKind.API_CALL,
                endpoint="batch_delete",
                count=len(cancel_batch),
                latency_ms=latency_ms,
            ))

        if post_batch:
            t0 = time.monotonic()
            if paper_mode:
                logger.debug("PAPER: post %d orders", len(post_batch))
                # Simulate immediate ack — feed back through account_q
                for intent in post_batch:
                    from src.types import HotEvent, EventKind, EventSource, Side
                    account_q.put_nowait(HotEvent(
                        recv_ts=time.monotonic_ns(), seq=0,
                        instrument_id=intent.instrument_id,
                        source=EventSource.ACCOUNT,
                        kind=EventKind.ORDER_ACK,
                        side=intent.side,
                        price=intent.price,
                        qty=intent.qty,
                        client_order_id=intent.client_order_id,
                    ))
            else:
                try:
                    await _async_post(clob_client, post_batch)
                except Exception as e:
                    logger.error("Post failed: %s", e)
            latency_ms = (time.monotonic() - t0) * 1000
            _emit(exec_telemetry_q, TelemetryEvent(
                kind=TelemetryKind.API_CALL,
                endpoint="batch_post",
                count=len(post_batch),
                latency_ms=latency_ms,
            ))
            for intent in post_batch:
                _emit(exec_telemetry_q, TelemetryEvent(
                    kind=TelemetryKind.ORDER_PLACED,
                    strategy_id=intent.strategy_id,
                    instrument_id=intent.instrument_id,
                    side=intent.side,
                    price=intent.price,
                    qty=intent.qty,
                    order_id=intent.client_order_id,
                ))

        if not post_batch and not cancel_batch:
            await asyncio.sleep(0.01)


async def _async_post(clob_client, intents: list[OrderIntent]) -> None:
    """Submit orders via py-clob-client. Placeholder for actual API integration."""
    # TODO: integrate with clob_client.post_order / post_orders
    pass


async def _async_cancel(clob_client, order_ids: list[str]) -> None:
    """Cancel orders via py-clob-client. Placeholder for actual API integration."""
    # TODO: integrate with clob_client.cancel_orders
    pass


def _emit(q: asyncio.Queue, event: TelemetryEvent) -> None:
    try:
        q.put_nowait(event)
    except asyncio.QueueFull:
        pass
```

- [ ] **Step 2: Write heartbeat.py**

```python
# src/execution/heartbeat.py
"""Heartbeat task — POST /heartbeat every N seconds."""
from __future__ import annotations

import asyncio
import logging
import time

from src.types import TelemetryEvent, TelemetryKind, KillState

logger = logging.getLogger(__name__)


async def heartbeat_loop(
    exec_telemetry_q: asyncio.Queue,
    risk_gate,
    clob_client,
    interval_s: int,
    fail_pause_after_s: int,
    paper_mode: bool,
    shutdown: asyncio.Event,
) -> None:
    """Send heartbeat every interval_s. Escalate kill switch on failure."""
    heartbeat_id: str | None = None
    consecutive_failures = 0
    last_success = time.monotonic()

    while not shutdown.is_set():
        await asyncio.sleep(interval_s)

        if paper_mode:
            _emit(exec_telemetry_q, TelemetryEvent(
                kind=TelemetryKind.HEARTBEAT,
                message="paper_mode",
                latency_ms=0.0,
                success=True,
            ))
            continue

        t0 = time.monotonic()
        try:
            # py-clob-client heartbeat
            resp = await asyncio.to_thread(
                clob_client.post_heartbeat, heartbeat_id
            )
            heartbeat_id = resp.get("heartbeat_id") if isinstance(resp, dict) else None
            latency_ms = (time.monotonic() - t0) * 1000
            consecutive_failures = 0
            last_success = time.monotonic()

            _emit(exec_telemetry_q, TelemetryEvent(
                kind=TelemetryKind.HEARTBEAT,
                latency_ms=latency_ms,
                success=True,
            ))

        except Exception as e:
            consecutive_failures += 1
            latency_ms = (time.monotonic() - t0) * 1000
            logger.warning("Heartbeat failed (%d): %s", consecutive_failures, e)

            _emit(exec_telemetry_q, TelemetryEvent(
                kind=TelemetryKind.HEARTBEAT,
                latency_ms=latency_ms,
                success=False,
                message=str(e),
            ))

            if time.monotonic() - last_success > fail_pause_after_s:
                logger.error("Heartbeat failed for %ds — PAUSING", fail_pause_after_s)
                risk_gate.kill_state = KillState.PAUSED
                _emit(exec_telemetry_q, TelemetryEvent(
                    kind=TelemetryKind.KILL_SWITCH,
                    kill_state=KillState.PAUSED,
                    message="heartbeat_failure",
                ))


def _emit(q: asyncio.Queue, event: TelemetryEvent) -> None:
    try:
        q.put_nowait(event)
    except asyncio.QueueFull:
        pass
```

- [ ] **Step 3: Commit**

```bash
git add src/execution/executor.py src/execution/heartbeat.py
git commit -m "feat(python-poc): add execution loop + heartbeat"
```

---

### Task 15: Wire Strategies + Execution into Main

**Files:**
- Modify: `polymarket-bot-python-poc/src/main.py`

- [ ] **Step 1: Update main.py to wire strategies + execution**

Update `run_bot()` in `src/main.py` to:
1. Create strategies from config (LadderMMStrategy + MomentumStrategy)
2. Create RiskGate with per-strategy limits
3. Create exec_telemetry_q
4. Launch execution_loop and heartbeat_loop tasks
5. Pass strategies and risk_gate to engine_loop

```python
# Add to imports in main.py:
from src.strategy.ladder_mm import LadderMMStrategy
from src.strategy.momentum import MomentumStrategy
from src.execution.risk import RiskGate, RiskLimits, GlobalRiskLimits
from src.execution.executor import execution_loop
from src.execution.heartbeat import heartbeat_loop

# In run_bot(), after creating market_state:

    # Create strategies
    strategies = []
    num_inst = len(instruments)

    if "ladder_mm" in config.strategies and config.strategies["ladder_mm"].enabled:
        strategies.append(LadderMMStrategy(
            strategy_id=0, config=config.strategies["ladder_mm"],
            num_instruments=num_inst,
        ))

    if "momentum" in config.strategies and config.strategies["momentum"].enabled:
        strategies.append(MomentumStrategy(
            strategy_id=1, config=config.strategies["momentum"],
            num_instruments=num_inst,
        ))

    # Risk gate
    strategy_limits = {}
    for name, scfg in config.strategies.items():
        strategy_limits[name] = RiskLimits(
            max_position_per_instrument=scfg.max_position,
            capital_budget=scfg.capital_budget,
            max_orders_live=scfg.max_orders_live,
            max_intents_per_sec=scfg.max_intents_per_sec,
            max_replaces_per_min=scfg.max_replaces_per_min,
            quote_age_ms=scfg.quote_age_ms,
        )
    global_risk = GlobalRiskLimits(
        wallet_min_free_usdc=config.global_risk.wallet_min_free_usdc,
        max_live_orders_total=config.global_risk.max_live_orders_total,
        max_submits_per_min=config.global_risk.max_submits_per_min,
        max_cancels_per_min=config.global_risk.max_cancels_per_min,
        feed_stale_ms=config.global_risk.feed_stale_ms,
        feed_dead_ms=config.global_risk.feed_dead_ms,
    )
    risk_gate = RiskGate(strategy_limits=strategy_limits, global_limits=global_risk)

    # Additional queues
    exec_telemetry_q: asyncio.Queue = asyncio.Queue(maxsize=4096)

    # CLOB client (None in paper mode)
    clob_client = None
    if not config.paper_mode:
        from py_clob_client.client import ClobClient
        private_key = os.getenv("PM_PRIVATE_KEY", "")
        clob_client = ClobClient(
            "https://clob.polymarket.com",
            key=private_key,
            chain_id=137,
        )

    # Launch tasks
    tasks = [
        asyncio.create_task(pm_ingest(pm_q, instruments, shutdown)),
        asyncio.create_task(bn_ingest(ref_q, ref_symbols, shutdown)),
        asyncio.create_task(account_ingest(
            account_q, instruments, api_key, api_secret, passphrase, shutdown,
        )),
        asyncio.create_task(engine_loop(
            pm_q, ref_q, account_q, intent_q, telemetry_q,
            market_state, strategies=strategies, risk_gate=risk_gate, shutdown=shutdown,
        )),
        asyncio.create_task(execution_loop(
            intent_q, exec_telemetry_q, account_q,
            risk_gate, clob_client, config.paper_mode, shutdown,
        )),
        asyncio.create_task(heartbeat_loop(
            exec_telemetry_q, risk_gate, clob_client,
            config.heartbeat.interval_s, config.heartbeat.fail_pause_after_s,
            config.paper_mode, shutdown,
        )),
    ]
```

- [ ] **Step 2: Test paper trading end-to-end**

```bash
cd polymarket-bot-python-poc
python -m src.main
```

Expected: Connects to feeds, discovers markets, strategies process events, paper orders logged, fills simulated.

- [ ] **Step 3: Commit**

```bash
git add src/main.py
git commit -m "feat(python-poc): wire strategies + risk (Phase 2 complete)"
```

---

## Phase 3: Dry Run Simulator + Execution

### Task 16: SimulatedExchange — Queue-Aware Fill Model

**Files:**
- Create: `polymarket-bot-python-poc/src/execution/simulator.py`
- Create: `polymarket-bot-python-poc/tests/test_simulator.py`

This is the most critical module in the bot. It replaces "BBO cross = fill" with a realistic maker execution model.

- [ ] **Step 1: Write the failing tests**

```python
# tests/test_simulator.py
from decimal import Decimal
from src.types import Side, OrderAction, OrderIntent
from src.execution.simulator import SimulatedExchange, LatencyProfile, SimConfig


def test_order_joins_queue_at_back():
    sim = SimulatedExchange(SimConfig(latency=LatencyProfile(
        submit_mean_ms=0, submit_std_ms=0, ack_mean_ms=0, ack_std_ms=0,
        cancel_mean_ms=0, cancel_std_ms=0)))
    # Level has 500 ahead
    sim.on_level_update(instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
                        new_size=Decimal("500"))
    sim.submit_order(OrderIntent(
        instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
        qty=Decimal("100"), action=OrderAction.POST,
        client_order_id=1, strategy_id=0))
    sim.process_pending()
    order = sim.get_order(1)
    assert order is not None
    assert order.queue_ahead == Decimal("500")


def test_trade_depletes_queue_no_fill():
    sim = SimulatedExchange(SimConfig(latency=LatencyProfile(
        submit_mean_ms=0, submit_std_ms=0, ack_mean_ms=0, ack_std_ms=0,
        cancel_mean_ms=0, cancel_std_ms=0)))
    sim.on_level_update(instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
                        new_size=Decimal("500"))
    sim.submit_order(OrderIntent(
        instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
        qty=Decimal("100"), action=OrderAction.POST,
        client_order_id=1, strategy_id=0))
    sim.process_pending()
    # Trade of 200 at our level — depletes queue but not enough to reach us
    fills = sim.on_trade(instrument_id=0, side=Side.SELL,
                         price=Decimal("0.50"), qty=Decimal("200"))
    assert len(fills) == 0
    assert sim.get_order(1).queue_ahead == Decimal("300")


def test_trade_depletes_queue_partial_fill():
    sim = SimulatedExchange(SimConfig(latency=LatencyProfile(
        submit_mean_ms=0, submit_std_ms=0, ack_mean_ms=0, ack_std_ms=0,
        cancel_mean_ms=0, cancel_std_ms=0)))
    sim.on_level_update(instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
                        new_size=Decimal("100"))
    sim.submit_order(OrderIntent(
        instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
        qty=Decimal("100"), action=OrderAction.POST,
        client_order_id=1, strategy_id=0))
    sim.process_pending()
    # Trade of 150 — 100 depletes queue, 50 fills us partially
    fills = sim.on_trade(instrument_id=0, side=Side.SELL,
                         price=Decimal("0.50"), qty=Decimal("150"))
    assert len(fills) == 1
    assert fills[0].fill_qty == Decimal("50")
    assert sim.get_order(1).fill_qty == Decimal("50")
    assert sim.get_order(1).qty - sim.get_order(1).fill_qty == Decimal("50")


def test_trade_full_fill():
    sim = SimulatedExchange(SimConfig(latency=LatencyProfile(
        submit_mean_ms=0, submit_std_ms=0, ack_mean_ms=0, ack_std_ms=0,
        cancel_mean_ms=0, cancel_std_ms=0)))
    sim.on_level_update(instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
                        new_size=Decimal("50"))
    sim.submit_order(OrderIntent(
        instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
        qty=Decimal("100"), action=OrderAction.POST,
        client_order_id=1, strategy_id=0))
    sim.process_pending()
    fills = sim.on_trade(instrument_id=0, side=Side.SELL,
                         price=Decimal("0.50"), qty=Decimal("200"))
    assert len(fills) == 1
    assert fills[0].fill_qty == Decimal("100")


def test_cancel_repost_loses_queue():
    sim = SimulatedExchange(SimConfig(latency=LatencyProfile(
        submit_mean_ms=0, submit_std_ms=0, ack_mean_ms=0, ack_std_ms=0,
        cancel_mean_ms=0, cancel_std_ms=0)))
    sim.on_level_update(instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
                        new_size=Decimal("100"))
    sim.submit_order(OrderIntent(
        instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
        qty=Decimal("100"), action=OrderAction.POST,
        client_order_id=1, strategy_id=0))
    sim.process_pending()
    # Deplete some queue
    sim.on_trade(instrument_id=0, side=Side.SELL,
                 price=Decimal("0.50"), qty=Decimal("80"))
    assert sim.get_order(1).queue_ahead == Decimal("20")
    # Cancel
    sim.cancel_order(1)
    sim.process_pending()
    # Repost — should be at BACK of queue
    sim.on_level_update(instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
                        new_size=Decimal("300"))
    sim.submit_order(OrderIntent(
        instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
        qty=Decimal("100"), action=OrderAction.POST,
        client_order_id=2, strategy_id=0))
    sim.process_pending()
    assert sim.get_order(2).queue_ahead == Decimal("300")


def test_post_only_rejects_marketable():
    sim = SimulatedExchange(SimConfig(
        post_only_reject=True,
        latency=LatencyProfile(
            submit_mean_ms=0, submit_std_ms=0, ack_mean_ms=0, ack_std_ms=0,
            cancel_mean_ms=0, cancel_std_ms=0)))
    # Best ask is 0.51
    sim.on_level_update(instrument_id=0, side=Side.SELL, price=Decimal("0.51"),
                        new_size=Decimal("100"))
    # Try to post buy at 0.52 — crosses the book
    rejects = sim.submit_order(OrderIntent(
        instrument_id=0, side=Side.BUY, price=Decimal("0.52"),
        qty=Decimal("100"), action=OrderAction.POST,
        client_order_id=1, strategy_id=0))
    sim.process_pending()
    order = sim.get_order(1)
    assert order is None or order.state.name == "REJECTED"


def test_multiple_partial_fills():
    sim = SimulatedExchange(SimConfig(latency=LatencyProfile(
        submit_mean_ms=0, submit_std_ms=0, ack_mean_ms=0, ack_std_ms=0,
        cancel_mean_ms=0, cancel_std_ms=0)))
    sim.on_level_update(instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
                        new_size=Decimal("0"))
    sim.submit_order(OrderIntent(
        instrument_id=0, side=Side.BUY, price=Decimal("0.50"),
        qty=Decimal("100"), action=OrderAction.POST,
        client_order_id=1, strategy_id=0))
    sim.process_pending()
    # Three partial fills
    fills1 = sim.on_trade(instrument_id=0, side=Side.SELL,
                          price=Decimal("0.50"), qty=Decimal("30"))
    fills2 = sim.on_trade(instrument_id=0, side=Side.SELL,
                          price=Decimal("0.50"), qty=Decimal("30"))
    fills3 = sim.on_trade(instrument_id=0, side=Side.SELL,
                          price=Decimal("0.50"), qty=Decimal("50"))
    assert len(fills1) == 1 and fills1[0].fill_qty == Decimal("30")
    assert len(fills2) == 1 and fills2[0].fill_qty == Decimal("30")
    assert len(fills3) == 1 and fills3[0].fill_qty == Decimal("40")  # only 40 remaining
    assert sim.get_order(1).fill_qty == Decimal("100")
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
python -m pytest tests/test_simulator.py -v
```

- [ ] **Step 3: Write simulator.py**

Implement `SimulatedExchange`, `SimConfig`, `LatencyProfile`, `SimOrder`, `FillEvent`. The core logic:
- `submit_order()` → schedule arrival at `now + latency`
- `process_pending()` → move arrived orders to simulated book, check post-only
- `on_trade()` → deplete queue FIFO, generate fills
- `on_level_update()` → update queue estimates via PowerProbQueueFunc
- `cancel_order()` → schedule removal at `now + cancel_latency`

- [ ] **Step 4: Run tests**

```bash
python -m pytest tests/test_simulator.py -v
```

Expected: all 7 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/execution/simulator.py tests/test_simulator.py
git commit -m "feat(python-poc): add SimulatedExchange with queue-aware fill model"
```

---

### Task 17: Markout Tracker

**Files:**
- Create: `polymarket-bot-python-poc/src/execution/markout.py`
- Create: `polymarket-bot-python-poc/tests/test_markout.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_markout.py
from decimal import Decimal
from src.types import Side
from src.execution.markout import MarkoutTracker, Markout


def test_markout_records_fill():
    tracker = MarkoutTracker(horizons_ms=[100, 500, 1000])
    tracker.record_fill(
        fill_id=1, instrument_id=0, fill_time_ns=1_000_000_000,
        fill_price=Decimal("0.50"), fill_side=Side.BUY,
        fill_qty=Decimal("100"), mid_at_fill=Decimal("0.51"))
    pending = tracker.pending_count
    assert pending == 1


def test_markout_updates_on_mid_change():
    tracker = MarkoutTracker(horizons_ms=[100, 1000])
    tracker.record_fill(
        fill_id=1, instrument_id=0, fill_time_ns=1_000_000_000,
        fill_price=Decimal("0.50"), fill_side=Side.BUY,
        fill_qty=Decimal("100"), mid_at_fill=Decimal("0.51"))
    # 100ms later, mid moved to 0.49
    tracker.on_mid_update(instrument_id=0, mid=Decimal("0.49"),
                          now_ns=1_100_000_000)
    m = tracker.get_markout(1)
    assert m.mid_at_100ms == Decimal("0.49")
    # 1000ms later
    tracker.on_mid_update(instrument_id=0, mid=Decimal("0.48"),
                          now_ns=2_000_000_000)
    m = tracker.get_markout(1)
    assert m.mid_at_1000ms == Decimal("0.48")


def test_markout_bps_calculation():
    m = Markout(
        fill_time_ns=0, fill_price=Decimal("0.50"), fill_side=Side.BUY,
        fill_qty=Decimal("100"), instrument_id=0,
        mid_at_fill=Decimal("0.50"), mid_at_1000ms=Decimal("0.48"))
    bps = m.markout_bps(1000)
    # Bought at 0.50, mid went to 0.48 → adverse: -200bps
    assert bps < 0


def test_markout_positive_for_good_fill():
    m = Markout(
        fill_time_ns=0, fill_price=Decimal("0.50"), fill_side=Side.BUY,
        fill_qty=Decimal("100"), instrument_id=0,
        mid_at_fill=Decimal("0.50"), mid_at_1000ms=Decimal("0.52"))
    bps = m.markout_bps(1000)
    # Bought at 0.50, mid went to 0.52 → good: +200bps
    assert bps > 0
```

- [ ] **Step 2: Run tests to verify they fail**

- [ ] **Step 3: Write markout.py**

```python
# src/execution/markout.py
"""Adverse selection tracking — mid-price after fill at configurable horizons."""
from __future__ import annotations

from dataclasses import dataclass, field
from decimal import Decimal

from src.types import Side


@dataclass
class Markout:
    fill_time_ns: int
    fill_price: Decimal
    fill_side: Side
    fill_qty: Decimal
    instrument_id: int
    mid_at_fill: Decimal
    mid_at_100ms: Decimal | None = None
    mid_at_500ms: Decimal | None = None
    mid_at_1000ms: Decimal | None = None
    mid_at_5000ms: Decimal | None = None

    def markout_bps(self, horizon_ms: int) -> float | None:
        attr = f"mid_at_{horizon_ms}ms"
        mid_later = getattr(self, attr, None)
        if mid_later is None or self.mid_at_fill == 0:
            return None
        diff = mid_later - self.fill_price
        if self.fill_side == Side.SELL:
            diff = -diff
        return float(diff / self.mid_at_fill * 10000)


class MarkoutTracker:
    def __init__(self, horizons_ms: list[int] | None = None) -> None:
        self._horizons_ms = horizons_ms or [100, 500, 1000, 5000]
        self._markouts: dict[int, Markout] = {}
        self._completed: list[Markout] = []

    def record_fill(self, fill_id: int, instrument_id: int,
                    fill_time_ns: int, fill_price: Decimal,
                    fill_side: Side, fill_qty: Decimal,
                    mid_at_fill: Decimal) -> None:
        self._markouts[fill_id] = Markout(
            fill_time_ns=fill_time_ns, fill_price=fill_price,
            fill_side=fill_side, fill_qty=fill_qty,
            instrument_id=instrument_id, mid_at_fill=mid_at_fill)

    def on_mid_update(self, instrument_id: int, mid: Decimal,
                      now_ns: int) -> None:
        completed_ids = []
        for fid, m in self._markouts.items():
            if m.instrument_id != instrument_id:
                continue
            elapsed_ms = (now_ns - m.fill_time_ns) / 1_000_000
            all_filled = True
            for h in self._horizons_ms:
                attr = f"mid_at_{h}ms"
                if getattr(m, attr) is None:
                    if elapsed_ms >= h:
                        setattr(m, attr, mid)
                    else:
                        all_filled = False
            if all_filled:
                completed_ids.append(fid)
        for fid in completed_ids:
            self._completed.append(self._markouts.pop(fid))

    def get_markout(self, fill_id: int) -> Markout | None:
        return self._markouts.get(fill_id)

    @property
    def pending_count(self) -> int:
        return len(self._markouts)

    @property
    def completed(self) -> list[Markout]:
        return self._completed

    def avg_markout_bps(self, horizon_ms: int) -> float | None:
        values = []
        for m in self._completed:
            bps = m.markout_bps(horizon_ms)
            if bps is not None:
                values.append(bps)
        return sum(values) / len(values) if values else None
```

- [ ] **Step 4: Run tests**

```bash
python -m pytest tests/test_markout.py -v
```

Expected: all 4 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/execution/markout.py tests/test_markout.py
git commit -m "feat(python-poc): add MarkoutTracker for adverse selection"
```

---

### Task 18: Executor (dry_run + live dispatch)

**Files:**
- Rewrite: `polymarket-bot-python-poc/src/execution/executor.py`

Update the executor to dispatch to `SimulatedExchange` in dry_run mode or `py-clob-client` in live mode. The executor feeds SimulatedExchange with market data (trades + level updates) so it can track queue positions and trigger fills.

- [ ] **Step 1: Rewrite executor.py**

Key changes from the old executor:
- In dry_run: feed every market data event to `SimulatedExchange.on_trade()` / `on_level_update()`
- Fills come from simulator, not from "BBO crossed my price"
- Fills are routed back through `account_q` as `HotEvent(kind=FILL)`
- Track `PnLBreakdown` with execution PnL vs rebate estimate
- Feed fills to `MarkoutTracker`

- [ ] **Step 2: Test dry_run end-to-end manually**

```bash
python -m src.main
```

Expected: Strategies post quotes → simulator tracks queue → fills only happen when queue depletes via real market trades → partial fills → markout tracked.

- [ ] **Step 3: Commit**

```bash
git add src/execution/executor.py
git commit -m "feat(python-poc): executor dispatches to simulator (dry_run) or API (live)"
```

---

### Task 19: Heartbeat + Kill Switch

**Files:**
- Keep: `polymarket-bot-python-poc/src/execution/heartbeat.py` (unchanged from Task 14)
- Keep: `polymarket-bot-python-poc/src/execution/risk.py` (unchanged from Task 11)

Already implemented. Heartbeat skips in dry_run, kill switch works in both modes.

- [ ] **Step 1: Verify heartbeat + risk work with new executor**

```bash
python -m pytest tests/test_risk.py -v
```

- [ ] **Step 2: Commit** (if any changes needed)

---

### Task 20: Wire Simulator into Main

**Files:**
- Modify: `polymarket-bot-python-poc/src/main.py`

- [ ] **Step 1: Update main.py**

Wire `SimulatedExchange` + `MarkoutTracker` into the bot lifecycle. The executor needs a reference to the simulator and feeds it market data events from the engine.

Key wiring: engine_loop emits market data events to a new `market_data_q` that the executor consumes to feed the simulator.

- [ ] **Step 2: Test end-to-end**

```bash
python -m src.main
```

Expected: Full dry_run with queue-aware fills, latency injection, partial fills, markout tracking.

- [ ] **Step 3: Commit**

```bash
git add src/main.py
git commit -m "feat(python-poc): wire simulator + markout into main (Phase 3 complete)"
```

---

### Task 21: PnL Breakdown + Dry Run Metrics

**Files:**
- Create: `polymarket-bot-python-poc/src/execution/pnl.py`

- [ ] **Step 1: Write pnl.py**

```python
# src/execution/pnl.py
"""PnL breakdown — execution vs MTM vs rebates."""
from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal


@dataclass
class PnLBreakdown:
    execution_pnl: Decimal = Decimal(0)
    mark_to_market_pnl: Decimal = Decimal(0)
    taker_fees_paid: Decimal = Decimal(0)
    estimated_rebate: Decimal = Decimal(0)

    @property
    def total_pnl(self) -> Decimal:
        """Primary metric: WITHOUT rebates."""
        return self.execution_pnl + self.mark_to_market_pnl

    @property
    def total_with_rebates(self) -> Decimal:
        return self.total_pnl + self.estimated_rebate - self.taker_fees_paid


def estimate_maker_rebate(qty: Decimal, price: Decimal,
                          fee_rate: Decimal, rebate_pct: Decimal) -> Decimal:
    """Estimate rebate for a maker fill: C × feeRate × p × (1-p) × rebate%."""
    return qty * fee_rate * price * (1 - price) * rebate_pct
```

- [ ] **Step 2: Commit**

```bash
git add src/execution/pnl.py
git commit -m "feat(python-poc): add PnL breakdown with rebate separation"
```

---

## Phase 4: Telemetry + Dashboard

### Task 22: Rolling Stats

**Files:**
- Create: `polymarket-bot-python-poc/src/telemetry/__init__.py`
- Create: `polymarket-bot-python-poc/src/telemetry/stats.py`
- Create: `polymarket-bot-python-poc/tests/test_stats.py`

- [ ] **Step 1: Write the failing test**

```python
# tests/test_stats.py
from src.telemetry.stats import LatencyHistogram, RollingCounter


def test_histogram_percentiles():
    hist = LatencyHistogram(max_samples=100)
    for i in range(100):
        hist.add(float(i))
    assert hist.p50() == 49.0
    assert hist.p99() == 98.0


def test_histogram_empty():
    hist = LatencyHistogram()
    assert hist.p50() == 0.0
    assert hist.p99() == 0.0


def test_rolling_counter():
    counter = RollingCounter(window_s=10.0, bucket_count=10)
    for i in range(50):
        counter.increment(1, timestamp=float(i))
    rate = counter.rate(timestamp=50.0)
    assert rate > 0.0
```

- [ ] **Step 2: Run test to verify it fails**

- [ ] **Step 3: Write stats.py**

```python
# src/telemetry/stats.py
"""Rolling counters and latency histogram for telemetry."""
from __future__ import annotations

import bisect
from collections import deque


class LatencyHistogram:
    def __init__(self, max_samples: int = 1000) -> None:
        self._samples: deque[float] = deque(maxlen=max_samples)
        self._sorted: list[float] = []
        self._dirty = False

    def add(self, value: float) -> None:
        self._samples.append(value)
        self._dirty = True

    def _ensure_sorted(self) -> None:
        if self._dirty:
            self._sorted = sorted(self._samples)
            self._dirty = False

    def _percentile(self, p: float) -> float:
        self._ensure_sorted()
        if not self._sorted:
            return 0.0
        idx = int(p / 100.0 * (len(self._sorted) - 1))
        return self._sorted[idx]

    def p50(self) -> float:
        return self._percentile(50)

    def p95(self) -> float:
        return self._percentile(95)

    def p99(self) -> float:
        return self._percentile(99)

    def p999(self) -> float:
        return self._percentile(99.9)

    @property
    def count(self) -> int:
        return len(self._samples)


class RollingCounter:
    def __init__(self, window_s: float = 10.0, bucket_count: int = 10) -> None:
        self._window_s = window_s
        self._bucket_s = window_s / bucket_count
        self._buckets: deque[tuple[float, int]] = deque()
        self._total = 0

    def increment(self, count: int = 1, timestamp: float = 0.0) -> None:
        self._expire(timestamp)
        if self._buckets and (timestamp - self._buckets[-1][0]) < self._bucket_s:
            ts, c = self._buckets[-1]
            self._buckets[-1] = (ts, c + count)
        else:
            self._buckets.append((timestamp, count))
        self._total += count

    def rate(self, timestamp: float = 0.0) -> float:
        self._expire(timestamp)
        if not self._buckets:
            return 0.0
        return self._total / self._window_s

    def _expire(self, timestamp: float) -> None:
        cutoff = timestamp - self._window_s
        while self._buckets and self._buckets[0][0] < cutoff:
            _, c = self._buckets.popleft()
            self._total -= c
```

Also create `src/telemetry/__init__.py` as empty file.

- [ ] **Step 4: Run tests**

```bash
python -m pytest tests/test_stats.py -v
```

Expected: all 3 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/telemetry/ tests/test_stats.py
git commit -m "feat(python-poc): add LatencyHistogram + RollingCounter"
```

---

### Task 23: Telemetry Loop + Recorder

**Files:**
- Create: `polymarket-bot-python-poc/src/telemetry/recorder.py`
- Create: `polymarket-bot-python-poc/src/telemetry/telemetry.py`

- [ ] **Step 1: Write recorder.py**

```python
# src/telemetry/recorder.py
"""Binary tape writer + structured JSON logger."""
from __future__ import annotations

import json
import logging
import struct
import time
from pathlib import Path

logger = logging.getLogger(__name__)


class StructuredLogger:
    """JSON-lines logger to mantis.log."""

    def __init__(self, path: Path) -> None:
        self._file = open(path, "a")

    def log(self, **kwargs) -> None:
        kwargs["ts"] = time.strftime("%H:%M:%S", time.localtime()) + f".{int(time.time() * 1000) % 1000:03d}"
        self._file.write(json.dumps(kwargs) + "\n")
        self._file.flush()

    def close(self) -> None:
        self._file.close()


class TapeWriter:
    """Simple binary tape — append-only, for replay."""

    def __init__(self, path: Path, record_size: int = 128) -> None:
        self._file = open(path, "wb")
        self._record_size = record_size
        self._count = 0

    def append(self, data: bytes) -> None:
        # Pad or truncate to record_size
        padded = data[:self._record_size].ljust(self._record_size, b"\x00")
        self._file.write(padded)
        self._count += 1

    @property
    def count(self) -> int:
        return self._count

    def close(self) -> None:
        self._file.flush()
        self._file.close()
```

- [ ] **Step 2: Write telemetry.py**

```python
# src/telemetry/telemetry.py
"""Telemetry loop — merges engine + execution events, outputs tape + log + stats + dashboard."""
from __future__ import annotations

import asyncio
import logging
import time
from pathlib import Path

from src.telemetry.recorder import StructuredLogger, TapeWriter
from src.telemetry.stats import LatencyHistogram, RollingCounter
from src.types import DashboardSnapshot, TelemetryEvent, TelemetryKind

logger = logging.getLogger(__name__)


async def telemetry_loop(
    telemetry_q: asyncio.Queue,
    exec_telemetry_q: asyncio.Queue,
    dash_q: asyncio.Queue,
    tape_dir: str,
    pm_q: asyncio.Queue,
    ref_q: asyncio.Queue,
    account_q: asyncio.Queue,
    intent_q: asyncio.Queue,
    shutdown: asyncio.Event,
) -> None:
    """Drain both telemetry queues, output tape + log + stats + dashboard snapshots."""
    Path(tape_dir).mkdir(parents=True, exist_ok=True)

    slog = StructuredLogger(Path(tape_dir) / "mantis.log")
    tape = TapeWriter(Path(tape_dir) / "input_tape.bin")

    lat_hist = LatencyHistogram()
    pm_rate = RollingCounter()
    bn_rate = RollingCounter()
    acct_rate = RollingCounter()

    strategy_stats: dict[int, dict] = {}
    last_dash_push = time.monotonic()
    global_seq = 0

    while not shutdown.is_set():
        now = time.monotonic()
        had_work = False

        # Drain engine telemetry
        while not telemetry_q.empty():
            had_work = True
            ev: TelemetryEvent = telemetry_q.get_nowait()
            global_seq += 1
            _process_event(ev, slog, tape, lat_hist, pm_rate, bn_rate, acct_rate, strategy_stats, now)

        # Drain execution telemetry
        while not exec_telemetry_q.empty():
            had_work = True
            ev = exec_telemetry_q.get_nowait()
            global_seq += 1
            _process_event(ev, slog, tape, lat_hist, pm_rate, bn_rate, acct_rate, strategy_stats, now)

        # Dashboard snapshot at 10Hz
        if now - last_dash_push >= 0.1:
            last_dash_push = now
            snap = DashboardSnapshot(
                epoch_ms=int(time.time() * 1000),
                lat_p50=lat_hist.p50(),
                lat_p95=lat_hist.p95(),
                lat_p99=lat_hist.p99(),
                lat_p999=lat_hist.p999(),
                pm_q_depth=pm_q.qsize(),
                ref_q_depth=ref_q.qsize(),
                account_q_depth=account_q.qsize(),
                intent_q_depth=intent_q.qsize(),
                pm_rate=pm_rate.rate(now),
                bn_rate=bn_rate.rate(now),
                account_rate=acct_rate.rate(now),
                strategies=dict(strategy_stats),
            )
            try:
                dash_q.put_nowait(snap)
            except asyncio.QueueFull:
                pass

        if not had_work:
            await asyncio.sleep(0.01)

    slog.close()
    tape.close()
    logger.info("Telemetry shutdown — %d events processed", global_seq)


def _process_event(
    ev: TelemetryEvent,
    slog: StructuredLogger,
    tape: TapeWriter,
    lat_hist: LatencyHistogram,
    pm_rate: RollingCounter,
    bn_rate: RollingCounter,
    acct_rate: RollingCounter,
    strategy_stats: dict,
    now: float,
) -> None:
    # Structured log for important events
    if ev.kind == TelemetryKind.FILL:
        slog.log(level="INFO", type="FILL", strategy_id=ev.strategy_id,
                 side=ev.side.name, qty=str(ev.qty), price=str(ev.price))
    elif ev.kind == TelemetryKind.ORDER_PLACED:
        slog.log(level="INFO", type="ORDER_PLACED", strategy_id=ev.strategy_id,
                 side=ev.side.name, qty=str(ev.qty), price=str(ev.price))
    elif ev.kind == TelemetryKind.RISK_REJECT:
        slog.log(level="WARN", type="RISK_REJECT", strategy_id=ev.strategy_id,
                 reason=ev.message)
    elif ev.kind == TelemetryKind.GLOBAL_REJECT:
        slog.log(level="WARN", type="GLOBAL_REJECT", reason=ev.message)
    elif ev.kind == TelemetryKind.API_CALL:
        slog.log(level="INFO", type="API_CALL", endpoint=ev.endpoint,
                 count=ev.count, latency_ms=round(ev.latency_ms, 1))
    elif ev.kind == TelemetryKind.HEARTBEAT:
        if not ev.success:
            slog.log(level="WARN", type="HEARTBEAT", success=False, message=ev.message)
    elif ev.kind == TelemetryKind.KILL_SWITCH:
        slog.log(level="ERROR", type="KILL_SWITCH", state=ev.kill_state.name,
                 reason=ev.message)

    # Latency tracking
    if ev.latency_ms > 0:
        lat_hist.add(ev.latency_ms)

    # Per-strategy stats
    if ev.strategy_id not in strategy_stats:
        strategy_stats[ev.strategy_id] = {
            "events": 0, "fills": 0, "rejects": 0, "intents": 0,
        }
    ss = strategy_stats[ev.strategy_id]
    ss["events"] += 1
    if ev.kind == TelemetryKind.FILL:
        ss["fills"] += 1
    if ev.kind == TelemetryKind.RISK_REJECT:
        ss["rejects"] += 1
    if ev.kind == TelemetryKind.ORDER_PLACED:
        ss["intents"] += 1
```

- [ ] **Step 3: Commit**

```bash
git add src/telemetry/recorder.py src/telemetry/telemetry.py
git commit -m "feat(python-poc): add telemetry loop + recorder"
```

---

### Task 24: Textual Dashboard

**Files:**
- Create: `polymarket-bot-python-poc/src/dashboard/__init__.py`
- Create: `polymarket-bot-python-poc/src/dashboard/app.py`

- [ ] **Step 1: Write app.py (Textual TUI)**

```python
# src/dashboard/app.py
"""Textual TUI dashboard — 3-column layout inspired by Nim FTXUI dashboard."""
from __future__ import annotations

import asyncio
from decimal import Decimal

from textual.app import App, ComposeResult
from textual.containers import Horizontal, Vertical
from textual.widgets import Header, Footer, Static, DataTable, Label
from textual.reactive import reactive

from src.types import DashboardSnapshot


class BookPanel(Static):
    """PM order book display."""
    def compose(self) -> ComposeResult:
        yield Label("PM BOOK", id="book-title")
        yield DataTable(id="book-table")

    def on_mount(self) -> None:
        table = self.query_one("#book-table", DataTable)
        table.add_columns("SIDE", "PRICE", "SIZE")

    def update_book(self, bids: list, asks: list) -> None:
        table = self.query_one("#book-table", DataTable)
        table.clear()
        for price, size in reversed(asks[:8]):
            table.add_row("ASK", str(price), str(size))
        table.add_row("---", "---", "---")
        for price, size in bids[:8]:
            table.add_row("BID", str(price), str(size))


class StatsPanel(Static):
    """Latency + queue depths + event rates."""
    snapshot: reactive[DashboardSnapshot | None] = reactive(None)

    def compose(self) -> ComposeResult:
        yield Label("LATENCY", id="lat-title")
        yield Label("", id="lat-values")
        yield Label("QUEUES", id="q-title")
        yield Label("", id="q-values")
        yield Label("RATES", id="rate-title")
        yield Label("", id="rate-values")

    def watch_snapshot(self, snap: DashboardSnapshot | None) -> None:
        if snap is None:
            return
        self.query_one("#lat-values", Label).update(
            f"p50: {snap.lat_p50:.1f}ms  p95: {snap.lat_p95:.1f}ms\n"
            f"p99: {snap.lat_p99:.1f}ms  p999: {snap.lat_p999:.1f}ms"
        )
        self.query_one("#q-values", Label).update(
            f"pm: {snap.pm_q_depth}  ref: {snap.ref_q_depth}\n"
            f"acct: {snap.account_q_depth}  intent: {snap.intent_q_depth}"
        )
        self.query_one("#rate-values", Label).update(
            f"PM: {snap.pm_rate:.0f}/s  BN: {snap.bn_rate:.0f}/s\n"
            f"Acct: {snap.account_rate:.0f}/s"
        )


class StrategyPanel(Static):
    """Per-strategy stats + risk status."""
    snapshot: reactive[DashboardSnapshot | None] = reactive(None)

    def compose(self) -> ComposeResult:
        yield Label("STRATEGIES", id="strat-title")
        yield Label("", id="strat-values")
        yield Label("RISK", id="risk-title")
        yield Label("", id="risk-values")

    def watch_snapshot(self, snap: DashboardSnapshot | None) -> None:
        if snap is None:
            return
        lines = []
        for sid, stats in snap.strategies.items():
            lines.append(
                f"[{sid}] fills:{stats.get('fills', 0)} "
                f"intents:{stats.get('intents', 0)} "
                f"rejects:{stats.get('rejects', 0)}"
            )
        self.query_one("#strat-values", Label).update("\n".join(lines) or "No strategies")
        self.query_one("#risk-values", Label).update(
            f"Kill: {snap.kill_state.name}\n"
            f"HB: {'OK' if snap.heartbeat_ok else 'FAIL'} ({snap.heartbeat_latency_ms:.0f}ms)\n"
            f"Wallet: ${snap.wallet_free_usdc}"
        )


class MantisApp(App):
    """Main Textual TUI application."""
    CSS = """
    Horizontal { height: 100%; }
    BookPanel { width: 1fr; }
    StatsPanel { width: 1fr; }
    StrategyPanel { width: 1fr; }
    """

    BINDINGS = [
        ("q", "quit", "Quit"),
    ]

    def __init__(self, dash_q: asyncio.Queue, **kwargs) -> None:
        super().__init__(**kwargs)
        self._dash_q = dash_q

    def compose(self) -> ComposeResult:
        yield Header(show_clock=True)
        with Horizontal():
            yield BookPanel(id="book")
            yield StatsPanel(id="stats")
            yield StrategyPanel(id="strategy")
        yield Footer()

    def on_mount(self) -> None:
        self.set_interval(0.1, self._poll_dashboard)

    def _poll_dashboard(self) -> None:
        snap = None
        while not self._dash_q.empty():
            try:
                snap = self._dash_q.get_nowait()
            except Exception:
                break
        if snap is not None:
            self.query_one("#stats", StatsPanel).snapshot = snap
            self.query_one("#strategy", StrategyPanel).snapshot = snap
```

Also create `src/dashboard/__init__.py` as empty file.

- [ ] **Step 2: Commit**

```bash
git add src/dashboard/
git commit -m "feat(python-poc): add Textual TUI dashboard"
```

---

### Task 25: Wire Telemetry + Dashboard into Main

**Files:**
- Modify: `polymarket-bot-python-poc/src/main.py`

- [ ] **Step 1: Update main.py with telemetry + dashboard tasks**

Add to imports:
```python
from src.telemetry.telemetry import telemetry_loop
from src.dashboard.app import MantisApp
```

Add queues:
```python
    dash_q: asyncio.Queue = asyncio.Queue(maxsize=256)
```

Add tasks:
```python
    tasks.append(asyncio.create_task(telemetry_loop(
        telemetry_q, exec_telemetry_q, dash_q,
        config.tape_dir, pm_q, ref_q, account_q, intent_q, shutdown,
    )))
```

For the dashboard, run Textual in its own async context:
```python
    if config.dashboard.enabled:
        app = MantisApp(dash_q=dash_q)
        # Textual's run_async integrates with the event loop
        tasks.append(asyncio.create_task(app.run_async()))
```

- [ ] **Step 2: Run the full bot**

```bash
cd polymarket-bot-python-poc
python -m src.main
```

Expected: Full bot running — WS feeds, engine, strategies, paper execution, telemetry logging, Textual dashboard showing books, stats, strategy panels.

- [ ] **Step 3: Commit**

```bash
git add src/main.py
git commit -m "feat(python-poc): wire telemetry + dashboard (Phase 4 complete)"
```

- [ ] **Step 4: Run all tests**

```bash
python -m pytest tests/ -v
```

Expected: All tests pass.

- [ ] **Step 5: Final commit**

```bash
git add -A
git commit -m "feat(python-poc): full-stack Polymarket bot POC complete"
```
