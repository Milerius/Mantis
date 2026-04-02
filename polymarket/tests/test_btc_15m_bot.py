import pytest
import sys
import os

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
    )
    assert m.window_close - m.window_open == 900
