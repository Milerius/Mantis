//! Trade summary statistics computed from a slice of [`TradeRecord`] values.
//!
//! The main entry point is [`compute_summary`], which returns a [`TradeSummary`]
//! capturing win-rate, P&L metrics, drawdown, Sharpe ratio, and profit factor.

use serde::{Deserialize, Serialize};

use pm_types::TradeRecord;

// ─── TradeSummary ────────────────────────────────────────────────────────────

/// Aggregated performance statistics for a set of closed trades.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradeSummary {
    /// Total number of closed trades.
    pub total_trades: u32,
    /// Number of profitable trades (`pnl > 0`).
    pub wins: u32,
    /// Number of unprofitable or break-even trades.
    pub losses: u32,
    /// Fraction of trades that were profitable (`wins / total_trades`).
    pub win_rate: f64,
    /// Sum of all realised P&L in USD.
    pub total_pnl: f64,
    /// Maximum peak-to-trough drawdown in USD (always `<= 0`).
    pub max_drawdown: f64,
    /// P&L of the single best trade.
    pub best_trade: f64,
    /// P&L of the single worst trade.
    pub worst_trade: f64,
    /// Mean P&L per trade.
    pub avg_pnl_per_trade: f64,
    /// Annualised Sharpe ratio (assuming ~96 fifteen-minute windows per day).
    pub sharpe_ratio: f64,
    /// Gross profit divided by gross loss (`0.0` when there are no losses).
    pub profit_factor: f64,
}

// ─── compute_summary ─────────────────────────────────────────────────────────

/// Annualisation factor: `sqrt(365 * 96)` where 96 is ~15-minute windows per day.
///
/// Pre-computed: `sqrt(365 * 96) ≈ 187.19`.
const ANNUALISATION_FACTOR: f64 = 187.189_743_308_761_45;

/// Compute a [`TradeSummary`] from a slice of closed trades.
///
/// Returns a zero-filled [`TradeSummary`] when `trades` is empty.
#[must_use]
pub fn compute_summary(trades: &[TradeRecord]) -> TradeSummary {
    if trades.is_empty() {
        return TradeSummary {
            total_trades: 0,
            wins: 0,
            losses: 0,
            win_rate: 0.0,
            total_pnl: 0.0,
            max_drawdown: 0.0,
            best_trade: 0.0,
            worst_trade: 0.0,
            avg_pnl_per_trade: 0.0,
            sharpe_ratio: 0.0,
            profit_factor: 0.0,
        };
    }

    let mut total_trades: u32 = 0;
    let mut wins: u32 = 0;
    let mut gross_profit = 0.0_f64;
    let mut gross_loss = 0.0_f64;
    let mut best_trade = f64::NEG_INFINITY;
    let mut worst_trade = f64::INFINITY;

    // Drawdown tracking
    let mut cumulative = 0.0_f64;
    let mut peak = 0.0_f64;
    let mut max_drawdown = 0.0_f64;

    let pnls: Vec<f64> = trades
        .iter()
        .map(|t| {
            total_trades += 1;
            let p = t.pnl.as_f64();
            if t.is_win() {
                wins += 1;
                gross_profit += p;
            } else {
                gross_loss += p.abs();
            }
            if p > best_trade {
                best_trade = p;
            }
            if p < worst_trade {
                worst_trade = p;
            }
            // Drawdown
            cumulative += p;
            if cumulative > peak {
                peak = cumulative;
            }
            let dd = cumulative - peak;
            if dd < max_drawdown {
                max_drawdown = dd;
            }
            p
        })
        .collect();

    let losses = total_trades - wins;
    let win_rate = f64::from(wins) / f64::from(total_trades);
    let total_pnl = pnls.iter().sum::<f64>();
    let avg_pnl_per_trade = total_pnl / f64::from(total_trades);

    // Sharpe ratio: mean / std * annualisation_factor
    let sharpe_ratio = {
        let mean = avg_pnl_per_trade;
        let variance = pnls
            .iter()
            .map(|p| {
                let diff = p - mean;
                diff * diff
            })
            .sum::<f64>()
            / f64::from(total_trades);
        let std_dev = variance.sqrt();
        if std_dev == 0.0 {
            0.0
        } else {
            (mean / std_dev) * ANNUALISATION_FACTOR
        }
    };

    let profit_factor = if gross_loss == 0.0 {
        0.0
    } else {
        gross_profit / gross_loss
    };

    TradeSummary {
        total_trades,
        wins,
        losses,
        win_rate,
        total_pnl,
        max_drawdown,
        best_trade,
        worst_trade,
        avg_pnl_per_trade,
        sharpe_ratio,
        profit_factor,
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use pm_types::{Asset, ContractPrice, OrderReason, Pnl, Side, StrategyId, WindowId};

    use super::*;

    fn make_record(window: u64, pnl: f64) -> TradeRecord {
        TradeRecord {
            window_id: WindowId::new(window),
            asset: Asset::Btc,
            side: Side::Up,
            entry_price: ContractPrice::new(0.45).expect("valid entry"),
            exit_price: ContractPrice::new(0.80).expect("valid exit"),
            size_usdc: 25.0,
            pnl: Pnl::new(pnl).expect("finite pnl"),
            opened_at_ms: 0,
            closed_at_ms: 3_600_000,
            close_reason: OrderReason::ExpiryClose,
            strategy_id: StrategyId::EarlyDirectional,
        }
    }

    #[test]
    fn empty_trades_returns_zeros() {
        let s = compute_summary(&[]);
        assert_eq!(s.total_trades, 0);
        assert_eq!(s.wins, 0);
        assert_eq!(s.losses, 0);
        assert_eq!(s.win_rate, 0.0);
        assert_eq!(s.total_pnl, 0.0);
        assert_eq!(s.max_drawdown, 0.0);
        assert_eq!(s.best_trade, 0.0);
        assert_eq!(s.worst_trade, 0.0);
        assert_eq!(s.avg_pnl_per_trade, 0.0);
        assert_eq!(s.sharpe_ratio, 0.0);
        assert_eq!(s.profit_factor, 0.0);
    }

    #[test]
    fn all_wins_no_drawdown() {
        let trades = vec![
            make_record(1, 10.0),
            make_record(2, 5.0),
            make_record(3, 20.0),
        ];
        let s = compute_summary(&trades);
        assert_eq!(s.total_trades, 3);
        assert_eq!(s.wins, 3);
        assert_eq!(s.losses, 0);
        assert_eq!(s.win_rate, 1.0);
        assert!((s.total_pnl - 35.0).abs() < 1e-10);
        assert_eq!(s.max_drawdown, 0.0, "all wins have no drawdown");
        assert!((s.best_trade - 20.0).abs() < 1e-10);
        assert!((s.worst_trade - 5.0).abs() < 1e-10);
        assert_eq!(s.profit_factor, 0.0, "no losses means profit_factor is 0");
    }

    #[test]
    fn mixed_trades_drawdown() {
        // Cumulative: 10 → 5 → 15 → 5
        // Peak sequence: 10 → 10 → 15 → 15
        // Drawdown sequence: 0 → -5 → 0 → -10
        // max_drawdown = -10
        let trades = vec![
            make_record(1, 10.0),
            make_record(2, -5.0),
            make_record(3, 10.0),
            make_record(4, -10.0),
        ];
        let s = compute_summary(&trades);
        assert_eq!(s.total_trades, 4);
        assert_eq!(s.wins, 2);
        assert_eq!(s.losses, 2);
        assert!((s.win_rate - 0.5).abs() < 1e-10);
        assert!((s.total_pnl - 5.0).abs() < 1e-10);
        assert!(
            (s.max_drawdown - (-10.0)).abs() < 1e-10,
            "max_drawdown={}",
            s.max_drawdown
        );
        assert!((s.best_trade - 10.0).abs() < 1e-10);
        assert!((s.worst_trade - (-10.0)).abs() < 1e-10);
    }

    #[test]
    fn profit_factor_correct() {
        // gross_profit = 10 + 20 = 30, gross_loss = 5 + 15 = 20
        // profit_factor = 30 / 20 = 1.5
        let trades = vec![
            make_record(1, 10.0),
            make_record(2, -5.0),
            make_record(3, 20.0),
            make_record(4, -15.0),
        ];
        let s = compute_summary(&trades);
        assert!(
            (s.profit_factor - 1.5).abs() < 1e-10,
            "profit_factor={}",
            s.profit_factor
        );
    }

    #[test]
    fn all_losses_profit_factor_zero() {
        let trades = vec![make_record(1, -5.0), make_record(2, -3.0)];
        let s = compute_summary(&trades);
        assert_eq!(s.wins, 0);
        assert_eq!(s.losses, 2);
        assert_eq!(s.profit_factor, 0.0);
        assert!(s.total_pnl < 0.0);
    }

    #[test]
    fn single_trade_win() {
        let trades = vec![make_record(1, 42.0)];
        let s = compute_summary(&trades);
        assert_eq!(s.total_trades, 1);
        assert_eq!(s.wins, 1);
        assert_eq!(s.losses, 0);
        assert_eq!(s.win_rate, 1.0);
        assert!((s.total_pnl - 42.0).abs() < 1e-10);
        assert_eq!(s.max_drawdown, 0.0);
        // Single trade with zero std dev → sharpe is 0
        assert_eq!(s.sharpe_ratio, 0.0);
    }

    #[test]
    fn sharpe_is_annualised() {
        // Two equal wins: mean=5, std=0 → sharpe=0
        let trades = vec![make_record(1, 5.0), make_record(2, 5.0)];
        let s = compute_summary(&trades);
        assert_eq!(s.sharpe_ratio, 0.0, "zero std dev should give sharpe=0");

        // Mixed: mean=0, std>0 → sharpe=0
        let trades2 = vec![make_record(1, 5.0), make_record(2, -5.0)];
        let s2 = compute_summary(&trades2);
        assert_eq!(s2.sharpe_ratio, 0.0, "zero mean should give sharpe=0");

        // Positive mean, positive std → positive sharpe
        let trades3 = vec![
            make_record(1, 10.0),
            make_record(2, 5.0),
            make_record(3, 8.0),
        ];
        let s3 = compute_summary(&trades3);
        assert!(
            s3.sharpe_ratio > 0.0,
            "positive skew should yield positive sharpe"
        );
    }

    #[test]
    fn annualisation_constant_correct() {
        // Verify the constant matches the formula sqrt(365 * 96).
        let expected = (365.0_f64 * 96.0_f64).sqrt();
        assert!((ANNUALISATION_FACTOR - expected).abs() < 1e-6);
    }
}
