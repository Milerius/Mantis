import json
import pytest
import sys
import os
import tempfile
import time
from pathlib import Path
from unittest.mock import patch, MagicMock

sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', 'scripts'))


def test_config_has_required_keys():
    from btc_15m_bot import CONFIG
    required = [
        "mode", "asset", "timeframe", "window_duration",
        "signal_delay_sec", "min_btc_delta_usd",
        "favored_prices", "favored_shares", "favored_max_price",
        "insurance_prices", "insurance_shares", "insurance_max_price",
        "insurance_start_pct", "stop_trading_pct", "max_side_switches",
        "max_deploy_per_window", "max_daily_loss", "max_consecutive_losses",
        "spy_enabled", "replay_dir",
        "micro_live_size", "spy_wallet", "spy_poll_interval_sec",
        "heisenberg_api_key", "log_file",
    ]
    for key in required:
        assert key in CONFIG, f"Missing config key: {key}"


def test_config_favored_prices_below_max():
    from btc_15m_bot import CONFIG
    for p in CONFIG["favored_prices"]:
        assert p <= CONFIG["favored_max_price"]


def test_config_insurance_prices_below_max():
    from btc_15m_bot import CONFIG
    for p in CONFIG["insurance_prices"]:
        assert p <= CONFIG["insurance_max_price"]


def test_fill_dataclass():
    from btc_15m_bot import Fill
    f = Fill(ts=1000.0, side="Up", price=0.55, shares=100.0, usdc=55.0, order_type="favored", order_id="test-1")
    assert f.side == "Up"
    assert f.usdc == 55.0


def test_position_pnl():
    from btc_15m_bot import Position
    pos = Position()
    pos.add_fill("Up", 100, 0.55, 55.0)
    pos.add_fill("Down", 50, 0.05, 2.5)
    assert pos.up_shares == 100
    assert pos.up_cost == 55.0
    assert pos.down_shares == 50
    assert pos.down_cost == 2.5
    assert pos.total_deployed == 57.5
    assert pos.pnl_if("Up") == pytest.approx(42.5)
    assert pos.pnl_if("Down") == pytest.approx(-7.5)


def test_market_dataclass():
    from btc_15m_bot import Market
    m = Market(
        condition_id="0xabc",
        token_up="token-up-123",
        token_down="token-down-456",
        slug="btc-updown-15m-1775140200",
        window_open=1775140200,
        window_close=1775141100,
        market_id=1819236,
    )
    assert m.window_close - m.window_open == 900
    assert m.market_id == 1819236


def test_window_manager_next_window():
    from btc_15m_bot import WindowManager
    wm = WindowManager(window_duration=900)

    # At timestamp 1775140500 (2.5 min into a window that opened at 1775140200)
    nxt = wm.next_window_open(now=1775140500)
    assert nxt == 1775141100  # next 15m boundary

    # At exact boundary
    nxt = wm.next_window_open(now=1775140200)
    assert nxt == 1775140200


def test_window_manager_pct_through():
    from btc_15m_bot import WindowManager
    import pytest
    wm = WindowManager(window_duration=900)
    wm.window_open = 1000
    wm.window_close = 1900

    assert wm.pct_through(1000) == 0.0
    assert wm.pct_through(1450) == pytest.approx(50.0)
    assert wm.pct_through(1900) == pytest.approx(100.0)


def test_window_manager_skip_mid_window():
    from btc_15m_bot import WindowManager
    wm = WindowManager(window_duration=900)

    # 10% into a window — should skip to next
    current_window_start = 1775140200
    now = current_window_start + 90  # 10% through
    nxt = wm.next_window_open(now=now)
    assert nxt == current_window_start + 900  # next window


def test_market_discovery_parses_gamma_response():
    from btc_15m_bot import MarketDiscovery, Market

    fake_event = {
        "slug": "btc-updown-15m-1775140200",
        "title": "Bitcoin Up or Down",
        "markets": [{
            "active": True,
            "closed": False,
            "conditionId": "0xabc123",
            "clobTokenIds": '["token-up-1", "token-down-2"]',
            "outcomes": '["Up", "Down"]',
            "endDate": "2026-04-02T14:45:00Z",
            "question": "BTC up or down 14:30-14:45?"
        }]
    }

    with patch("btc_15m_bot.requests.get") as mock_get:
        mock_resp = MagicMock()
        mock_resp.json.return_value = [fake_event]
        mock_resp.raise_for_status = MagicMock()
        mock_get.return_value = mock_resp

        md = MarketDiscovery(asset="btc")
        market = md.find_market(window_open=1775140200)

        assert market is not None
        assert market.token_up == "token-up-1"
        assert market.token_down == "token-down-2"
        assert market.condition_id == "0xabc123"
        assert market.slug == "btc-updown-15m-1775140200"


def test_market_discovery_returns_none_when_no_match():
    from btc_15m_bot import MarketDiscovery

    with patch("btc_15m_bot.requests.get") as mock_get:
        mock_resp = MagicMock()
        mock_resp.json.return_value = []
        mock_resp.raise_for_status = MagicMock()
        mock_get.return_value = mock_resp

        md = MarketDiscovery(asset="btc")
        market = md.find_market(window_open=1775140200)
        assert market is None


def test_signal_engine_direction_up():
    from btc_15m_bot import SignalEngine
    se = SignalEngine(min_delta=10.0)
    se.open_price = 84000.0
    se.current_price = 84050.0
    direction = se.compute_direction()
    assert direction == "Up"


def test_signal_engine_direction_down():
    from btc_15m_bot import SignalEngine
    se = SignalEngine(min_delta=10.0)
    se.open_price = 84000.0
    se.current_price = 83920.0
    direction = se.compute_direction()
    assert direction == "Down"


def test_signal_engine_skip_on_small_delta():
    from btc_15m_bot import SignalEngine
    se = SignalEngine(min_delta=10.0)
    se.open_price = 84000.0
    se.current_price = 84005.0
    direction = se.compute_direction()
    assert direction is None


def test_signal_engine_delta():
    from btc_15m_bot import SignalEngine
    import pytest
    se = SignalEngine(min_delta=10.0)
    se.open_price = 84000.0
    se.current_price = 84123.45
    assert se.delta() == pytest.approx(123.45)


def test_paper_executor_immediate_fill_when_crossing_spread():
    from btc_15m_bot import PaperExecutor
    fake_book = {"asks": [{"price": "0.55", "size": "200"}], "bids": []}
    with patch("btc_15m_bot.requests.get") as mock_get:
        mock_resp = MagicMock()
        mock_resp.json.return_value = fake_book
        mock_get.return_value = mock_resp
        ex = PaperExecutor()
        oid = ex.place_gtc_order("token-1", "BUY", 0.60, 100)
        fills = ex.get_fills()
        assert len(fills) == 1
        assert fills[0][1] == 0.55   # filled at ask, not our price
        assert fills[0][2] == 100    # full fill


def test_paper_executor_resting_order_no_fill():
    from btc_15m_bot import PaperExecutor
    fake_book = {"asks": [{"price": "0.70", "size": "200"}], "bids": []}
    with patch("btc_15m_bot.requests.get") as mock_get:
        mock_resp = MagicMock()
        mock_resp.json.return_value = fake_book
        mock_get.return_value = mock_resp
        ex = PaperExecutor()
        oid = ex.place_gtc_order("token-1", "BUY", 0.55, 100)
        fills = ex.get_fills()
        assert len(fills) == 0
        assert oid in ex.get_open_orders()


def test_paper_executor_cancel():
    from btc_15m_bot import PaperExecutor
    fake_book = {"asks": [{"price": "0.70", "size": "200"}], "bids": []}
    with patch("btc_15m_bot.requests.get") as mock_get:
        mock_resp = MagicMock()
        mock_resp.json.return_value = fake_book
        mock_get.return_value = mock_resp
        ex = PaperExecutor()
        oid = ex.place_gtc_order("token-1", "BUY", 0.55, 100)
        assert oid in ex.get_open_orders()
        ex.cancel_order(oid)
        assert oid not in ex.get_open_orders()


def test_paper_executor_cancel_all():
    from btc_15m_bot import PaperExecutor
    fake_book = {"asks": [{"price": "0.90", "size": "200"}], "bids": []}
    with patch("btc_15m_bot.requests.get") as mock_get:
        mock_resp = MagicMock()
        mock_resp.json.return_value = fake_book
        mock_get.return_value = mock_resp
        ex = PaperExecutor()
        ex.place_gtc_order("token-1", "BUY", 0.50, 100)
        ex.place_gtc_order("token-1", "BUY", 0.55, 100)
        assert len(ex.get_open_orders()) == 2
        ex.cancel_all()
        assert len(ex.get_open_orders()) == 0


def test_paper_executor_tick_fills_resting_order():
    from btc_15m_bot import PaperExecutor
    # First: place resting order (ask too high)
    high_ask_book = {"asks": [{"price": "0.80", "size": "200"}], "bids": []}
    # Second: on tick, ask drops to our price
    low_ask_book = {"asks": [{"price": "0.50", "size": "200"}], "bids": []}

    with patch("btc_15m_bot.requests.get") as mock_get:
        # First call: placement (high ask, no fill)
        mock_resp_high = MagicMock()
        mock_resp_high.json.return_value = high_ask_book
        mock_get.return_value = mock_resp_high

        ex = PaperExecutor()
        oid = ex.place_gtc_order("token-1", "BUY", 0.55, 100)
        assert len(ex.get_fills()) == 0
        assert oid in ex.get_open_orders()

        # Now tick with lower ask
        mock_resp_low = MagicMock()
        mock_resp_low.json.return_value = low_ask_book
        mock_get.return_value = mock_resp_low

        ex.tick()
        fills = ex.get_fills()
        assert len(fills) == 1
        assert fills[0][1] == 0.50  # filled at new ask
        assert fills[0][2] == 100
        assert oid not in ex.get_open_orders()  # removed after fill


def test_micro_live_executor_caps_shares():
    from btc_15m_bot import MicroLiveExecutor
    with patch("btc_15m_bot.ClobClient") as MockClient:
        mock_client = MagicMock()
        mock_client.create_order.return_value = MagicMock()
        mock_client.post_order.return_value = {"orderID": "live-1", "status": "live"}
        MockClient.return_value = mock_client

        ex = MicroLiveExecutor(private_key="0xfake", micro_size=1.0)
        ex._client = mock_client
        oid = ex.place_gtc_order("token-1", "BUY", 0.55, 100)

        call_args = mock_client.create_order.call_args
        order_args = call_args[0][0]
        assert order_args.size == 1.0


def test_live_executor_uses_full_size():
    from btc_15m_bot import LiveExecutor
    with patch("btc_15m_bot.ClobClient") as MockClient:
        mock_client = MagicMock()
        mock_client.create_order.return_value = MagicMock()
        mock_client.post_order.return_value = {"orderID": "live-2", "status": "live"}
        MockClient.return_value = mock_client

        ex = LiveExecutor(private_key="0xfake")
        ex._client = mock_client
        oid = ex.place_gtc_order("token-1", "BUY", 0.55, 100)

        call_args = mock_client.create_order.call_args
        order_args = call_args[0][0]
        assert order_args.size == 100


def test_order_manager_rejects_above_max_price():
    from btc_15m_bot import OrderManager, PaperExecutor, Position, CONFIG
    fake_book = {"asks": [{"price": "0.90", "size": "200"}], "bids": []}
    with patch("btc_15m_bot.requests.get") as mock_get:
        mock_resp = MagicMock()
        mock_resp.json.return_value = fake_book
        mock_get.return_value = mock_resp
        ex = PaperExecutor()
        pos = Position()
        om = OrderManager(executor=ex, position=pos, config=CONFIG)
        placed = om.place_favored("token-up", 0.80, 100)
        assert placed is False
        assert len(ex.get_open_orders()) == 0


def test_order_manager_rejects_over_budget():
    from btc_15m_bot import OrderManager, PaperExecutor, Position, CONFIG
    fake_book = {"asks": [{"price": "0.50", "size": "5000"}], "bids": []}
    with patch("btc_15m_bot.requests.get") as mock_get:
        mock_resp = MagicMock()
        mock_resp.json.return_value = fake_book
        mock_get.return_value = mock_resp
        ex = PaperExecutor()
        pos = Position()
        config = dict(CONFIG)
        config["max_deploy_per_window"] = 100
        om = OrderManager(executor=ex, position=pos, config=config)
        assert om.place_favored("token-up", 0.50, 100) is True
        pos.add_fill("Up", 100, 0.50, 50.0)
        assert om.place_favored("token-up", 0.60, 100) is False


def test_order_manager_posts_full_ladder():
    from btc_15m_bot import OrderManager, PaperExecutor, Position, CONFIG
    fake_book = {"asks": [{"price": "0.90", "size": "5000"}], "bids": []}
    with patch("btc_15m_bot.requests.get") as mock_get:
        mock_resp = MagicMock()
        mock_resp.json.return_value = fake_book
        mock_get.return_value = mock_resp
        ex = PaperExecutor()
        pos = Position()
        om = OrderManager(executor=ex, position=pos, config=CONFIG)
        om.post_favored_ladder("token-up", "Up")
        assert len(ex.get_open_orders()) == len(CONFIG["favored_prices"])


def test_order_manager_posts_insurance():
    from btc_15m_bot import OrderManager, PaperExecutor, Position, CONFIG
    fake_book = {"asks": [{"price": "0.90", "size": "5000"}], "bids": []}
    with patch("btc_15m_bot.requests.get") as mock_get:
        mock_resp = MagicMock()
        mock_resp.json.return_value = fake_book
        mock_get.return_value = mock_resp
        ex = PaperExecutor()
        pos = Position()
        om = OrderManager(executor=ex, position=pos, config=CONFIG)
        om.post_insurance("token-down")
        assert len(ex.get_open_orders()) == len(CONFIG["insurance_prices"])


def test_window_recorder_writes_json():
    from btc_15m_bot import WindowRecorder, Market, Position
    with tempfile.TemporaryDirectory() as tmpdir:
        rec = WindowRecorder(replay_dir=tmpdir)
        market = Market(
            condition_id="0xabc", token_up="t1", token_down="t2",
            slug="btc-updown-15m-1775140200",
            window_open=1775140200, window_close=1775141100,
        )
        pos = Position()
        pos.add_fill("Up", 100, 0.55, 55.0)
        signal_data = {"btc_open_price": 84000, "btc_at_signal": 84050,
                       "delta": 50, "direction": "Up"}
        rec.write(market=market, position=pos, winner="Up",
                  signal=signal_data, spy_data=None)

        files = list(Path(tmpdir).glob("*.json"))
        assert len(files) == 1
        data = json.loads(files[0].read_text())
        assert data["window"]["slug"] == "btc-updown-15m-1775140200"
        assert data["our_position"]["pnl"] == pytest.approx(45.0)
        assert data["window"]["winner"] == "Up"


def test_settlement_handler_resolves_winner():
    from btc_15m_bot import SettlementHandler
    with patch("btc_15m_bot.requests.get") as mock_get:
        mock_resp = MagicMock()
        mock_resp.status_code = 200
        mock_resp.json.return_value = {
            "id": 1819236,
            "outcomes": '["Up", "Down"]',
            "outcomePrices": '["1", "0"]',
            "closed": True,
        }
        mock_get.return_value = mock_resp

        sh = SettlementHandler()
        winner = sh.resolve(slug="btc-updown-15m-1775140200", condition_id="0xabc",
                           market_id=1819236)
        assert winner == "Up"


def test_spy_thread_populates_data():
    from btc_15m_bot import SpyThread
    fake_trades = [
        {"outcome": "Down", "price": 0.55, "size": 100, "side": "BUY",
         "timestamp": "2026-04-02T14:30:20Z", "slug": "btc-updown-15m-1775140200"},
        {"outcome": "Up", "price": 0.05, "size": 50, "side": "BUY",
         "timestamp": "2026-04-02T14:38:00Z", "slug": "btc-updown-15m-1775140200"},
    ]
    with patch("btc_15m_bot.requests.post") as mock_post:
        mock_resp = MagicMock()
        mock_resp.status_code = 200
        mock_resp.json.return_value = {"data": {"results": fake_trades}}
        mock_post.return_value = mock_resp

        spy = SpyThread(
            wallet="0xtest",
            api_key="fake-key",
            window_open=1775140200,
            window_close=1775141100,
            slug="btc-updown-15m-1775140200",
            poll_interval=999,
        )
        spy._poll_once()
        data = spy.get_data()
        assert data["direction"] == "Down"
        assert data["down_cost"] > 0
        assert data["up_cost"] > 0


def test_run_one_window_paper_mode(tmp_path):
    from btc_15m_bot import run_one_window, CONFIG, Market

    config = dict(CONFIG)
    config["mode"] = "paper"
    config["replay_dir"] = str(tmp_path)
    config["signal_delay_sec"] = 0
    config["spy_enabled"] = False

    market = Market(
        condition_id="0xabc", token_up="tok-up", token_down="tok-down",
        slug="btc-updown-15m-1775140200",
        window_open=1775140200, window_close=1775141100,
    )

    fake_book = {"asks": [{"price": "0.55", "size": "5000"}], "bids": []}

    with patch("btc_15m_bot.requests.get") as mock_get:
        mock_resp = MagicMock()
        mock_resp.json.return_value = fake_book
        mock_resp.raise_for_status = MagicMock()
        mock_get.return_value = mock_resp

        result = run_one_window(
            config=config,
            market=market,
            direction="Up",
            signal_data={"btc_open_price": 84000, "btc_at_signal": 84050,
                         "delta": 50, "direction": "Up"},
            winner="Up",
        )

        assert result["deployed"] > 0
        assert result["pnl"] != 0
        replays = list(tmp_path.glob("*.json"))
        assert len(replays) == 1


def test_settlement_resolves_via_market_id():
    from btc_15m_bot import SettlementHandler
    with patch("btc_15m_bot.requests.get") as mock_get:
        mock_resp = MagicMock()
        mock_resp.status_code = 200
        mock_resp.json.return_value = {
            "id": 1819236,
            "outcomes": '["Up", "Down"]',
            "outcomePrices": '["1", "0"]',
            "closed": True,
        }
        mock_get.return_value = mock_resp

        sh = SettlementHandler()
        winner = sh.resolve(slug="btc-updown-15m-123", condition_id="0xabc",
                           market_id=1819236)
        assert winner == "Up"
        # Verify it called the right URL
        mock_get.assert_called_once()
        call_url = mock_get.call_args[0][0]
        assert "1819236" in call_url


def test_settlement_resolves_down_winner():
    from btc_15m_bot import SettlementHandler
    with patch("btc_15m_bot.requests.get") as mock_get:
        mock_resp = MagicMock()
        mock_resp.status_code = 200
        mock_resp.json.return_value = {
            "outcomes": '["Up", "Down"]',
            "outcomePrices": '["0", "1"]',
            "closed": True,
        }
        mock_get.return_value = mock_resp

        sh = SettlementHandler()
        winner = sh.resolve(slug="x", condition_id="0x1", market_id=123)
        assert winner == "Down"


def test_settlement_retries_when_not_settled():
    from btc_15m_bot import SettlementHandler
    with patch("btc_15m_bot.requests.get") as mock_get:
        # First call: prices are equal (not settled yet)
        unsettled = MagicMock()
        unsettled.status_code = 200
        unsettled.json.return_value = {
            "outcomes": '["Up", "Down"]',
            "outcomePrices": '["0.5", "0.5"]',
            "closed": False,
        }
        # Second call: resolved
        settled = MagicMock()
        settled.status_code = 200
        settled.json.return_value = {
            "outcomes": '["Up", "Down"]',
            "outcomePrices": '["1", "0"]',
            "closed": True,
        }
        mock_get.side_effect = [unsettled, settled]

        sh = SettlementHandler()
        winner = sh.resolve(slug="x", condition_id="0x1", market_id=99,
                           retries=2, delay=0)  # delay=0 for fast test
        assert winner == "Up"
        assert mock_get.call_count == 2


def test_settlement_fallback_to_orderbook():
    from btc_15m_bot import SettlementHandler
    with patch("btc_15m_bot.requests.get") as mock_get:
        # Market API returns unsettled
        unsettled = MagicMock()
        unsettled.status_code = 200
        unsettled.json.return_value = {
            "outcomes": '["Up", "Down"]',
            "outcomePrices": '["0.5", "0.5"]',
        }
        # Orderbook check: Up token has bid at 0.95
        up_book = MagicMock()
        up_book.status_code = 200
        up_book.json.return_value = {"bids": [{"price": "0.95", "size": "1000"}], "asks": []}

        mock_get.side_effect = [unsettled] * 3 + [up_book]

        sh = SettlementHandler()
        winner = sh.resolve(slug="x", condition_id="0x1", market_id=99,
                           token_up="tok-up", token_down="tok-down",
                           retries=3, delay=0)
        assert winner == "Up"


def test_market_dataclass_has_market_id():
    from btc_15m_bot import Market
    m = Market(
        condition_id="0xabc", token_up="t1", token_down="t2",
        slug="btc-updown-15m-123", window_open=100, window_close=1000,
        market_id=1819236,
    )
    assert m.market_id == 1819236

    # market_id is optional
    m2 = Market(
        condition_id="0xabc", token_up="t1", token_down="t2",
        slug="test", window_open=100, window_close=1000,
    )
    assert m2.market_id is None
