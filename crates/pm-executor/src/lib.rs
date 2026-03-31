//! Execution engines: backtest, paper, and live trading modes.

#![deny(unsafe_code)]

pub mod backtest;
pub mod paper;
pub mod sweep;

pub use backtest::{
    BacktestConfig, BacktestResult, BacktestResultV2, ContractPriceProvider, FixedPriceProvider,
    InstanceResultV2, ModelPriceProvider, run_backtest, run_backtest_v2,
};
pub use paper::{PaperConfig, PaperExecutor};
pub use sweep::{SweepConfig, SweepResult, run_sweep};
