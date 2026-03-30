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

extern crate std;

use std::string::String;
use std::vec;
use std::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::asset::{Asset, Timeframe};

// ─── StrategyConfig ──────────────────────────────────────────────────────────

/// Per-strategy configuration loaded from `[[bot.strategies]]` TOML blocks.
///
/// Each variant maps to a concrete strategy in `pm-signal`.  The `type` field
/// in the TOML table selects the variant; remaining fields are the parameters.
///
/// Example:
/// ```toml
/// [[bot.strategies]]
/// type = "early_directional"
/// max_entry_time_secs = 180
/// min_spot_magnitude   = 0.001
/// max_entry_price      = 0.58
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StrategyConfig {
    /// Parameters for [`pm_signal::EarlyDirectional`].
    EarlyDirectional {
        /// Human-readable label to distinguish variants (e.g. "tight", "loose").
        #[serde(default)]
        label: String,
        /// Maximum seconds elapsed since window open to still enter.
        max_entry_time_secs: u64,
        /// Minimum absolute spot move fraction required (e.g. `0.001` = 0.1 %).
        min_spot_magnitude: f64,
        /// Maximum contract ask price to accept (e.g. `0.58`).
        max_entry_price: f64,
    },
    /// Parameters for [`pm_signal::MomentumConfirmation`].
    MomentumConfirmation {
        /// Human-readable label to distinguish variants (e.g. "tight", "loose").
        #[serde(default)]
        label: String,
        /// Earliest seconds elapsed before this strategy activates.
        min_entry_time_secs: u64,
        /// Latest seconds elapsed after which this strategy no longer fires.
        max_entry_time_secs: u64,
        /// Minimum absolute spot move fraction required (e.g. `0.003` = 0.3 %).
        min_spot_magnitude: f64,
        /// Maximum contract ask price to accept (e.g. `0.72`).
        max_entry_price: f64,
    },
    /// Parameters for [`pm_signal::CompleteSetArb`].
    CompleteSetArb {
        /// Maximum acceptable combined ask (Up + Down) to trigger entry.
        max_combined_cost: f64,
        /// Minimum profit-per-share required (i.e. `1 - combined`).
        min_profit_per_share: f64,
    },
    /// Parameters for [`pm_signal::HedgeLock`].
    HedgeLock {
        /// Maximum combined cost (entry + hedge ask) to still enter.
        max_combined_cost: f64,
    },
}

/// Default strategy list — mirrors the hardcoded values used before
/// configurable strategies were introduced.
///
/// Applied automatically when `strategies` is absent from the TOML.
#[must_use]
pub fn default_strategies() -> Vec<StrategyConfig> {
    vec![
        StrategyConfig::EarlyDirectional {
            label: String::new(),
            max_entry_time_secs: 180,
            min_spot_magnitude: 0.001,
            max_entry_price: 0.58,
        },
        StrategyConfig::MomentumConfirmation {
            label: String::new(),
            min_entry_time_secs: 180,
            max_entry_time_secs: 480,
            min_spot_magnitude: 0.003,
            max_entry_price: 0.72,
        },
        StrategyConfig::CompleteSetArb {
            max_combined_cost: 0.98,
            min_profit_per_share: 0.01,
        },
        StrategyConfig::HedgeLock {
            max_combined_cost: 0.95,
        },
    ]
}

fn default_max_positions_per_window() -> usize {
    1
}
fn default_scan_interval_secs() -> u64 {
    120
}
fn default_max_price_age_ms() -> u64 {
    15_000
}

// ─── TrendFilterConfig ──────────────────────────────────────────────────────

/// Trend filter configuration.
///
/// Controls the EMA-based higher-timeframe trend filter that prevents
/// trading against the prevailing trend.  Particularly important for
/// short (5 m) windows where signal-to-noise is low.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrendFilterConfig {
    /// Enable/disable the trend filter.
    #[serde(default = "default_trend_filter_enabled")]
    pub enabled: bool,
    /// Fast EMA period in ticks (default: 20, ~10 min at 2 ticks/sec).
    #[serde(default = "default_trend_fast_period")]
    pub fast_period: usize,
    /// Slow EMA period in ticks (default: 60, ~30 min).
    #[serde(default = "default_trend_slow_period")]
    pub slow_period: usize,
    /// Minimum trend strength to consider the trend established (default: 0.0005).
    #[serde(default = "default_min_trend_strength")]
    pub min_trend_strength: f64,
}

impl Default for TrendFilterConfig {
    fn default() -> Self {
        Self {
            enabled: default_trend_filter_enabled(),
            fast_period: default_trend_fast_period(),
            slow_period: default_trend_slow_period(),
            min_trend_strength: default_min_trend_strength(),
        }
    }
}

fn default_trend_filter_enabled() -> bool {
    true
}
fn default_trend_fast_period() -> usize {
    20
}
fn default_trend_slow_period() -> usize {
    60
}
fn default_min_trend_strength() -> f64 {
    0.0005
}

// ─── EntryTimingConfig ──────────────────────────────────────────────────────

/// Configuration for smart entry timing.
///
/// When enabled, the bot waits after a signal fires for optimal conditions
/// (spread improvement, better ask price) before executing, up to a timeout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntryTimingConfig {
    /// Enable/disable smart entry timing (default: false — opt-in).
    #[serde(default = "default_entry_timing_enabled")]
    pub enabled: bool,
    /// Maximum seconds to wait for optimal conditions after signal fires.
    #[serde(default = "default_max_wait_secs")]
    pub max_wait_secs: u64,
    /// Minimum spread improvement (fraction narrower than at signal time) to trigger early entry.
    #[serde(default = "default_min_spread_improvement")]
    pub min_spread_improvement: f64,
}

impl Default for EntryTimingConfig {
    fn default() -> Self {
        Self {
            enabled: default_entry_timing_enabled(),
            max_wait_secs: default_max_wait_secs(),
            min_spread_improvement: default_min_spread_improvement(),
        }
    }
}

fn default_entry_timing_enabled() -> bool {
    false
}
fn default_max_wait_secs() -> u64 {
    5
}
fn default_min_spread_improvement() -> f64 {
    0.02
}

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
    /// Strategy parameter sets to enable.
    ///
    /// If absent from the TOML, the four built-in defaults are used so that
    /// old config files continue to work without modification.
    #[serde(default = "default_strategies")]
    pub strategies: Vec<StrategyConfig>,
    /// Maximum positions per window (default: 1).
    #[serde(default = "default_max_positions_per_window")]
    pub max_positions_per_window: usize,
    /// Market scan interval in seconds (default: 30).
    #[serde(default = "default_scan_interval_secs")]
    pub scan_interval_secs: u64,
    /// Maximum age of cached prices in milliseconds before fallback (default: 15000).
    #[serde(default = "default_max_price_age_ms")]
    pub max_price_age_ms: u64,
    /// Higher-timeframe EMA trend filter configuration.
    #[serde(default)]
    pub trend_filter: TrendFilterConfig,
    /// Smart entry timing configuration.
    #[serde(default)]
    pub entry_timing: EntryTimingConfig,
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
    extern crate std;
    use std::string::ToString;
    use std::vec;

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
        // Absent strategies → defaults applied.
        assert_eq!(cfg.bot.strategies.len(), 4);
    }

    #[test]
    fn strategies_default_when_absent() {
        let cfg: BotConfig =
            toml::from_str(DEFAULT_TOML).expect("valid TOML should deserialize");
        // No [[bot.strategies]] block → default_strategies() is used.
        let defaults = default_strategies();
        assert_eq!(cfg.bot.strategies, defaults);
    }

    #[test]
    fn deserialize_explicit_strategies() {
        let toml = r#"
[bot]
mode = "backtest"
min_edge = 0.03
max_position_usdc = 25.0
max_total_exposure_usdc = 500.0
max_daily_loss_usdc = 100.0
kelly_fraction = 0.25
assets = []

[[bot.strategies]]
type = "early_directional"
max_entry_time_secs = 120
min_spot_magnitude = 0.002
max_entry_price = 0.55

[[bot.strategies]]
type = "complete_set_arb"
max_combined_cost = 0.97
min_profit_per_share = 0.02

[backtest]
start_date = "2025-01-01"
end_date = "2025-06-01"
initial_balance = 500.0
slippage_bps = 10

[data]
cache_dir = "data"
log_dir = "logs"
"#;
        let cfg: BotConfig = toml::from_str(toml).expect("strategies should deserialize");
        assert_eq!(cfg.bot.strategies.len(), 2);
        assert_eq!(
            cfg.bot.strategies[0],
            StrategyConfig::EarlyDirectional {
                label: String::new(),
                max_entry_time_secs: 120,
                min_spot_magnitude: 0.002,
                max_entry_price: 0.55,
            }
        );
        assert_eq!(
            cfg.bot.strategies[1],
            StrategyConfig::CompleteSetArb {
                max_combined_cost: 0.97,
                min_profit_per_share: 0.02,
            }
        );
    }

    #[test]
    fn deserialize_all_strategy_variants() {
        let toml = r#"
[bot]
mode = "backtest"
min_edge = 0.03
max_position_usdc = 25.0
max_total_exposure_usdc = 500.0
max_daily_loss_usdc = 100.0
kelly_fraction = 0.25
assets = []

[[bot.strategies]]
type = "early_directional"
max_entry_time_secs = 180
min_spot_magnitude = 0.001
max_entry_price = 0.58

[[bot.strategies]]
type = "momentum_confirmation"
min_entry_time_secs = 180
max_entry_time_secs = 480
min_spot_magnitude = 0.003
max_entry_price = 0.72

[[bot.strategies]]
type = "complete_set_arb"
max_combined_cost = 0.98
min_profit_per_share = 0.01

[[bot.strategies]]
type = "hedge_lock"
max_combined_cost = 0.95

[backtest]
start_date = "2025-01-01"
end_date = "2025-06-01"
initial_balance = 500.0
slippage_bps = 10

[data]
cache_dir = "data"
log_dir = "logs"
"#;
        let cfg: BotConfig = toml::from_str(toml).expect("all strategy variants should deserialize");
        assert_eq!(cfg.bot.strategies.len(), 4);
        assert_eq!(cfg.bot.strategies, default_strategies());
    }
}
