//! Bot configuration types for TOML deserialization.
//!
//! This module is only available when the `std` feature is enabled, because
//! TOML deserialization requires heap allocation.
//!
//! The expected configuration file format matches `config/default.toml`.
//! Load with:
//! ```ignore
//! let src = std::fs::read_to_string("config/default.toml")?;
//! let cfg: BotConfig = toml::from_str(&src)?;
//! ```

use serde::{Deserialize, Serialize};

use crate::asset::{Asset, Timeframe};

// ─── Mode ────────────────────────────────────────────────────────────────────

/// Operating mode of the bot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Replay historical data; no real orders placed.
    Backtest,
    /// Use live market data but submit paper (simulated) orders.
    Paper,
    /// Fully live: real data, real orders, real money.
    Live,
}

impl core::fmt::Display for Mode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Backtest => write!(f, "backtest"),
            Self::Paper => write!(f, "paper"),
            Self::Live => write!(f, "live"),
        }
    }
}

// ─── AssetConfig ─────────────────────────────────────────────────────────────

/// Per-asset configuration block inside `[bot]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetConfig {
    /// Which asset this block configures.
    pub asset: Asset,
    /// Whether trading is enabled for this asset.
    pub enabled: bool,
    /// Timeframes to generate signals for this asset.
    pub timeframes: Vec<Timeframe>,
}

// ─── BacktestConfig ──────────────────────────────────────────────────────────

/// Configuration for the backtesting engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BacktestConfig {
    /// ISO-8601 date string for the start of the backtest window.
    pub start_date: String,
    /// ISO-8601 date string for the end of the backtest window.
    pub end_date: String,
    /// Starting USDC balance for the simulated account.
    pub initial_balance: f64,
    /// Simulated slippage in basis points applied to each fill.
    pub slippage_bps: u32,
}

// ─── DataConfig ──────────────────────────────────────────────────────────────

/// Paths for cached data and log output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataConfig {
    /// Directory where downloaded OHLCV data is cached.
    pub cache_dir: String,
    /// Directory where structured log files are written.
    pub log_dir: String,
}

// ─── BotSection ──────────────────────────────────────────────────────────────

/// The `[bot]` section of the configuration file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BotSection {
    /// Operating mode.
    pub mode: Mode,
    /// Minimum edge required to place a bet (fractional, e.g. `0.03` = 3 %).
    pub min_edge: f64,
    /// Maximum USDC allocated to a single open position.
    pub max_position_usdc: f64,
    /// Maximum total USDC across all open positions.
    pub max_total_exposure_usdc: f64,
    /// Maximum USDC loss tolerated within a single calendar day.
    pub max_daily_loss_usdc: f64,
    /// Kelly fraction applied to raw Kelly sizing (e.g. `0.25` = quarter-Kelly).
    pub kelly_fraction: f64,
    /// Per-asset configuration blocks.
    pub assets: Vec<AssetConfig>,
}

// ─── BotConfig ───────────────────────────────────────────────────────────────

/// Top-level configuration for the Polymarket trading bot.
///
/// Deserializes directly from `config/default.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BotConfig {
    /// Core bot parameters and asset list.
    pub bot: BotSection,
    /// Backtesting parameters.
    pub backtest: BacktestConfig,
    /// Data / logging paths.
    pub data: DataConfig,
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// TOML that mirrors `config/default.toml` exactly.
    const DEFAULT_TOML: &str = r#"
[bot]
mode = "backtest"
min_edge = 0.03
max_position_usdc = 25.0
max_total_exposure_usdc = 500.0
max_daily_loss_usdc = 100.0
kelly_fraction = 0.25

[[bot.assets]]
asset = "btc"
enabled = true
timeframes = ["min5", "min15", "hour1", "hour4"]

[[bot.assets]]
asset = "eth"
enabled = true
timeframes = ["min5", "min15", "hour1", "hour4"]

[[bot.assets]]
asset = "sol"
enabled = false
timeframes = ["min5", "min15", "hour1", "hour4"]

[[bot.assets]]
asset = "xrp"
enabled = false
timeframes = ["min5", "min15", "hour1", "hour4"]

[backtest]
start_date = "2025-10-01"
end_date = "2026-03-28"
initial_balance = 500.0
slippage_bps = 10

[data]
cache_dir = "data"
log_dir = "logs"
"#;

    #[test]
    fn deserialize_default_toml() {
        let cfg: BotConfig = toml::from_str(DEFAULT_TOML).expect("valid TOML should deserialize");

        // Mode
        assert_eq!(cfg.bot.mode, Mode::Backtest);

        // Bot section scalars
        assert!((cfg.bot.min_edge - 0.03).abs() < f64::EPSILON);
        assert!((cfg.bot.max_position_usdc - 25.0).abs() < f64::EPSILON);
        assert!((cfg.bot.max_total_exposure_usdc - 500.0).abs() < f64::EPSILON);
        assert!((cfg.bot.max_daily_loss_usdc - 100.0).abs() < f64::EPSILON);
        assert!((cfg.bot.kelly_fraction - 0.25).abs() < f64::EPSILON);

        // Assets
        assert_eq!(cfg.bot.assets.len(), 4);

        let btc = &cfg.bot.assets[0];
        assert_eq!(btc.asset, Asset::Btc);
        assert!(btc.enabled);
        assert_eq!(
            btc.timeframes,
            vec![
                Timeframe::Min5,
                Timeframe::Min15,
                Timeframe::Hour1,
                Timeframe::Hour4
            ]
        );

        let sol = &cfg.bot.assets[2];
        assert_eq!(sol.asset, Asset::Sol);
        assert!(!sol.enabled);

        // Backtest
        assert_eq!(cfg.backtest.start_date, "2025-10-01");
        assert_eq!(cfg.backtest.end_date, "2026-03-28");
        assert!((cfg.backtest.initial_balance - 500.0).abs() < f64::EPSILON);
        assert_eq!(cfg.backtest.slippage_bps, 10);

        // Data
        assert_eq!(cfg.data.cache_dir, "data");
        assert_eq!(cfg.data.log_dir, "logs");
    }

    #[test]
    fn mode_display() {
        assert_eq!(Mode::Backtest.to_string(), "backtest");
        assert_eq!(Mode::Paper.to_string(), "paper");
        assert_eq!(Mode::Live.to_string(), "live");
    }

    #[test]
    fn deserialize_paper_mode() {
        let toml = r#"
[bot]
mode = "paper"
min_edge = 0.05
max_position_usdc = 10.0
max_total_exposure_usdc = 100.0
max_daily_loss_usdc = 50.0
kelly_fraction = 0.5
assets = []

[backtest]
start_date = "2025-01-01"
end_date = "2025-06-01"
initial_balance = 1000.0
slippage_bps = 5

[data]
cache_dir = "cache"
log_dir = "logs"
"#;
        let cfg: BotConfig = toml::from_str(toml).expect("paper mode should deserialize");
        assert_eq!(cfg.bot.mode, Mode::Paper);
        assert!(cfg.bot.assets.is_empty());
    }
}
