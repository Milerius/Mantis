//! Polymarket trading bot CLI.

mod backtest;
mod calibrate;
mod download;
mod paper;
mod pbt_backtest;
mod pbt_download;
mod sweep;

use anyhow::{Context as _, Result};
use clap::{Parser, Subcommand};
use pm_types::config::BotConfig;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

// ─── CLI definition ───────────────────────────────────────────────────────────

/// Polymarket crypto Up/Down trading bot.
#[derive(Parser)]
#[command(name = "polymarket", about = "Polymarket crypto Up/Down trading bot")]
struct Cli {
    /// Path to config file.
    #[arg(short, long, default_value = "config/default.toml")]
    config: String,

    /// Subcommand to execute.
    #[command(subcommand)]
    command: Commands,
}

/// Available CLI subcommands.
#[derive(Subcommand)]
enum Commands {
    /// Download historical price data from Binance and Polymarket.
    Download,
    /// Calibrate fair-value model from historical data.
    Calibrate,
    /// Run backtest on test-set data using a calibrated model.
    Backtest,
    /// Run parameter sweep to find optimal strategy configuration.
    Sweep,
    /// Run download + calibrate + backtest in one step.
    Run,
    /// Download `PolyBackTest` historical snapshot data.
    PbtDownload,
    /// Run backtest using real `PolyBackTest` contract prices.
    PbtBacktest,
    /// Run paper trading with live WebSocket feeds.
    Paper,
}

// ─── Config loader ────────────────────────────────────────────────────────────

fn load_config(path: &str) -> Result<BotConfig> {
    let src = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read config file `{path}`"))?;
    toml::from_str(&src).with_context(|| format!("cannot parse config file `{path}`"))
}

// ─── main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    // Initialise structured logging (respects RUST_LOG env var).
    fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let cli = Cli::parse();
    let cfg = load_config(&cli.config)?;
    info!(mode = %cfg.bot.mode, config = %cli.config, "bot starting");

    match cli.command {
        Commands::Download => {
            download::run_download(&cfg).await?;
        }

        Commands::Calibrate => {
            let (train_dates, _test_dates) = calibrate::split_dates(&cfg)?;
            let _result = calibrate::run_calibrate(&cfg, &train_dates)?;
            info!("calibration finished — lookup table and contract model ready");
        }

        Commands::Backtest => {
            let (train_dates, test_dates) = calibrate::split_dates(&cfg)?;
            let result = calibrate::run_calibrate(&cfg, &train_dates)?;
            backtest::run_backtest_cmd(&cfg, result, &test_dates)?;
        }

        Commands::Sweep => {
            sweep::run_sweep_cmd(&cfg)?;
        }

        Commands::Run => {
            download::run_download(&cfg).await?;
            let (train_dates, test_dates) = calibrate::split_dates(&cfg)?;
            let result = calibrate::run_calibrate(&cfg, &train_dates)?;
            backtest::run_backtest_cmd(&cfg, result, &test_dates)?;
        }

        Commands::PbtDownload => {
            pbt_download::run_pbt_download(&cfg).await?;
        }

        Commands::PbtBacktest => {
            pbt_backtest::run_pbt_backtest(&cfg)?;
        }

        Commands::Paper => {
            paper::run_paper(&cfg).await?;
        }
    }

    Ok(())
}
