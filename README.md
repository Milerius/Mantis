# Polymarket Trading Bot

Automated crypto prediction market trading bot for Polymarket binary options, built in Rust.

## Architecture

The workspace contains 7 crates plus a top-level binary:

| Crate | Purpose |
|---|---|
| **pm-types** | Newtypes (`Price`, `ContractPrice`, `Edge`, `Pnl`), enums (`Asset`, `Timeframe`, `Side`), and TOML config deserialization. `no_std`-compatible core. |
| **pm-signal** | Strategy engine with pluggable strategies. Evaluates market state each tick and emits trade decisions. |
| **pm-oracle** | Spot price feeds from Binance and OKX WebSockets, plus PolyBackTest historical data pipeline. Deduplicates ticks via `OracleRouter`. |
| **pm-market** | Polymarket CLOB integration: market scanner (Gamma API), WebSocket orderbook tracker, and dynamic token subscription. |
| **pm-risk** | Risk manager: Kelly sizing, per-position and total exposure limits, daily loss kill switch. |
| **pm-executor** | Order execution: backtest simulator, paper executor (simulated fills with slippage), and future live CLOB client. |
| **pm-bookkeeper** | Trade logging, P&L summaries, CSV/JSON export, and live tick+orderbook snapshot recording. |

The top-level binary (`src/main.rs`) wires these together via CLI subcommands.

## Quick Start

```bash
# Build
cargo build

# Run all tests
cargo test --workspace --lib

# Run backtest with default config
cargo run -- backtest

# Run backtest with real PolyBackTest orderbook data
cargo run -- pbt-backtest

# Run paper trading (requires live WebSocket connections)
cargo run -- paper --config config/production.toml

# Parameter sweep (1,944 strategy combos)
cargo run -- sweep
```

## Configuration

Configuration lives in `config/production.toml`. Key sections:

```toml
[bot]
mode = "paper"              # backtest | paper | live
min_edge = 0.03             # Minimum edge to place a bet (3%)
max_position_usdc = 25.0    # Max USDC per position
max_total_exposure_usdc = 500.0
max_daily_loss_usdc = 100.0 # Kill switch threshold
kelly_fraction = 0.25       # Quarter-Kelly sizing

[[bot.assets]]
asset = "btc"
enabled = true
timeframes = ["min5", "min15"]

[[bot.strategies]]
type = "early_directional"
max_entry_time_secs = 150
min_spot_magnitude = 0.002
max_entry_price = 0.53
```

All fields in `[bot]` support serde defaults, so old TOML files remain forward-compatible when new fields are added.

## Strategies

### EarlyDirectional (primary edge)

Enters within the first few minutes of a prediction window when spot price has moved decisively. Targets cheap contracts (<$0.53-0.58) before the market prices in the move. Backtested at 78% win rate across 700+ trades.

### MomentumConfirmation

Activates after the early window (3-8 minutes) when the spot move has sustained and strengthened. Accepts higher entry prices (up to $0.72) in exchange for higher directional confidence. 80.5% win rate.

### CompleteSetArb

Buys both Up and Down contracts when combined cost < $0.98, guaranteeing profit since one side always pays $1.00. Currently disabled -- requires two-leg atomic execution.

### HedgeLock

Similar to complete set arb but triggered when an existing position can be hedged at favorable combined cost. Also disabled pending two-leg execution support.

## CLI Commands

| Command | Description |
|---|---|
| `download` | Fetch Binance + Polymarket historical data |
| `calibrate` | Build fair-value models from historical data |
| `backtest` | Run backtest with model prices |
| `sweep` | Parameter sweep across strategy configurations |
| `pbt-download` | Download PolyBackTest real orderbook snapshots |
| `pbt-backtest` | Backtest against real PBT orderbook data |
| `paper` | Live paper trading with WebSocket feeds |

## Current Status

- **Backtest**: 749 trades, 78% win rate, +$19,834 P&L, 5.81 profit factor
- **Paper trading**: Operational with Binance + OKX + Polymarket WebSocket feeds
- **Live trading**: Not yet implemented (Phase 3 -- requires CLOB client + EIP-712 signing)

### Limitations

- Live execution (real orders) not yet implemented
- CompleteSetArb and HedgeLock disabled (need two-leg atomic execution)
- No volatility regime filter yet (planned improvement)
- Position sizes capped at $25 during paper validation phase

## License

Apache-2.0
