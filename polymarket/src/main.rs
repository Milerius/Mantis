//! Polymarket trading bot CLI.

use clap::{Parser, Subcommand};

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
    /// Download historical price data from Binance and OKX.
    Download,
    /// Calibrate fair_value model from historical data.
    Calibrate,
    /// Run backtest on historical data.
    Backtest,
    /// Run download + calibrate + backtest in one step.
    Run,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Download => eprintln!("download: not yet implemented"),
        Commands::Calibrate => eprintln!("calibrate: not yet implemented"),
        Commands::Backtest => eprintln!("backtest: not yet implemented"),
        Commands::Run => eprintln!("run: not yet implemented"),
    }
}
