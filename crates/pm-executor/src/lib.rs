//! Execution engines: backtest, paper, and live trading modes.

#![deny(unsafe_code)]

pub mod backtest;
pub mod paper;
pub mod sweep;

pub use backtest::{
    BacktestConfig, BacktestResult, ContractPriceProvider, FixedPriceProvider, ModelPriceProvider,
    run_backtest,
};
pub use paper::{PaperConfig, PaperExecutor};
pub use sweep::{SweepConfig, SweepResult, run_sweep};
