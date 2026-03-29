//! Execution engines: backtest, paper, and live trading modes.

#![deny(unsafe_code)]

pub mod backtest;

pub use backtest::{BacktestConfig, BacktestResult, run_backtest};
