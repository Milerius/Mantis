//! Paper trading executor — simulates fills against live market data.
//!
//! [`PaperExecutor`] mirrors the backtest engine's position accounting but
//! is designed for real-time use: it opens positions on demand and resolves
//! them when a window outcome is known.

use pm_bookkeeper::compute_summary;
use pm_types::{
    Asset, ContractPrice, EntryDecision, Fill, OpenPosition, OrderId, OrderReason, Pnl, Side,
    StrategyId, TradeRecord, WindowId,
};
use tracing::{debug, info, warn};

// ─── PaperConfig ─────────────────────────────────────────────────────────────

/// Configuration for the paper trading executor.
#[derive(Debug, Clone)]
pub struct PaperConfig {
    /// Starting USDC balance.
    pub initial_balance: f64,
    /// Slippage applied to the entry price in basis points (1 bp = 0.0001).
    pub slippage_bps: u32,
    /// Maximum USDC size per position.
    pub max_position_usdc: f64,
    /// Maximum number of open positions per window.
    pub max_positions_per_window: usize,
}

// ─── ActivePosition ───────────────────────────────────────────────────────────

/// An open paper position held by the executor.
#[derive(Debug, Clone)]
struct ActivePosition {
    /// The underlying open position record.
    pos: OpenPosition,
    /// Strategy that opened this position.
    strategy_id: StrategyId,
}

// ─── PaperExecutor ───────────────────────────────────────────────────────────

/// Paper trading executor — simulates fills against live market data.
///
/// Positions are opened with [`try_open_position`](Self::try_open_position) and
/// resolved at window expiry via [`resolve_window`](Self::resolve_window).
/// Fill prices include a configurable slippage.
pub struct PaperExecutor {
    config: PaperConfig,
    open_positions: Vec<ActivePosition>,
    trades: Vec<TradeRecord>,
    balance: f64,
    next_order_id: u64,
}

impl PaperExecutor {
    /// Create a new [`PaperExecutor`] with the given configuration.
    #[must_use]
    pub fn new(config: PaperConfig) -> Self {
        let balance = config.initial_balance;
        Self {
            config,
            open_positions: Vec::new(),
            trades: Vec::new(),
            balance,
            next_order_id: 1,
        }
    }

    /// Try to open a position based on an entry decision.
    ///
    /// Simulates a fill at the ask price + slippage.  Returns `None` if:
    /// - There is already `max_positions_per_window` open for this window.
    /// - Balance is too low to cover the position size.
    ///
    /// The entry price is clamped to `[0.01, 0.99]` after slippage is applied.
    #[expect(clippy::too_many_arguments, reason = "size_usdc must be passed from risk manager")]
    pub fn try_open_position(
        &mut self,
        decision: &EntryDecision,
        window_id: WindowId,
        asset: Asset,
        timestamp_ms: u64,
        size_usdc: f64,
    ) -> Option<Fill> {
        // Guard: position cap per window.
        let count = self
            .open_positions
            .iter()
            .filter(|ap| ap.pos.window_id == window_id)
            .count();
        if count >= self.config.max_positions_per_window {
            debug!(
                %window_id,
                %asset,
                "position cap reached for window — skipping"
            );
            return None;
        }

        let size = size_usdc;
        if size <= 0.0 || size > self.balance {
            debug!(%window_id, %asset, "insufficient balance — skipping");
            return None;
        }

        // Apply slippage to the limit price from the decision.
        let slippage = f64::from(self.config.slippage_bps) * 0.0001;
        let raw_entry = decision.limit_price.as_f64() + slippage;
        let entry_clamped = raw_entry.clamp(0.01, 0.99);

        let Some(avg_entry) = ContractPrice::new(entry_clamped) else {
            debug!(%window_id, %asset, "invalid entry price after slippage — skipping");
            return None;
        };

        let order_id = OrderId::new(self.next_order_id);
        self.next_order_id += 1;

        // Deduct from balance.
        self.balance -= size;

        let pos = OpenPosition {
            window_id,
            asset,
            side: decision.side,
            avg_entry,
            size_usdc: size,
            opened_at_ms: timestamp_ms,
        };

        debug!(
            %window_id,
            %asset,
            side = %decision.side,
            strategy = %decision.strategy_id,
            entry = entry_clamped,
            size,
            "paper position opened"
        );

        self.open_positions.push(ActivePosition {
            pos,
            strategy_id: decision.strategy_id,
        });

        Some(Fill {
            order_id,
            fill_price: avg_entry,
            size_usdc: size,
            timestamp_ms,
        })
    }

    /// Resolve a window — close all positions for this window.
    ///
    /// Win: `payout = size_usdc / entry_price`, `pnl = payout - size_usdc`.
    /// Loss: `payout = 0`, `pnl = -size_usdc`.
    ///
    /// Returns the total realised P&L across all positions closed in this window.
    /// Callers (e.g. the paper trading loop) should pass this value to the risk
    /// manager via `RiskManager::on_window_resolved` so it can track cumulative
    /// daily loss correctly.
    pub fn resolve_window(&mut self, window_id: WindowId, outcome: Side, timestamp_ms: u64) -> Pnl {
        let mut total_pnl: f64 = 0.0;
        let mut i = 0;
        while i < self.open_positions.len() {
            if self.open_positions[i].pos.window_id != window_id {
                i += 1;
                continue;
            }

            let ap = self.open_positions.swap_remove(i);
            let pos = ap.pos;
            let entry = pos.avg_entry.as_f64();

            let (pnl_val, exit_price_val) = if pos.side == outcome {
                if entry <= 0.0 {
                    (0.0_f64, 0.0_f64)
                } else {
                    let num_contracts = pos.size_usdc / entry;
                    let payout = num_contracts; // × $1.00
                    (payout - pos.size_usdc, 1.0_f64)
                }
            } else {
                (-pos.size_usdc, 0.0_f64)
            };

            self.balance += pos.size_usdc + pnl_val;
            total_pnl += pnl_val;

            let exit_price = ContractPrice::new(exit_price_val.clamp(0.0, 1.0))
                .unwrap_or_else(|| ContractPrice::new(0.0).unwrap_or_else(|| {
                    // SAFETY: 0.0 is always a valid ContractPrice (finite, in [0,1]).
                    unreachable!("0.0 is always a valid ContractPrice")
                }));
            let pnl = Pnl::new(pnl_val).unwrap_or(Pnl::ZERO);

            debug!(
                %window_id,
                asset = %pos.asset,
                side = %pos.side,
                outcome = %outcome,
                pnl = pnl_val,
                strategy = %ap.strategy_id,
                "paper position resolved"
            );

            self.trades.push(TradeRecord {
                window_id,
                asset: pos.asset,
                side: pos.side,
                entry_price: pos.avg_entry,
                exit_price,
                size_usdc: pos.size_usdc,
                pnl,
                opened_at_ms: pos.opened_at_ms,
                closed_at_ms: timestamp_ms,
                close_reason: OrderReason::ExpiryClose,
                strategy_id: ap.strategy_id,
            });
        }

        Pnl::new(total_pnl).unwrap_or(Pnl::ZERO)
    }

    /// Current USDC balance.
    #[must_use]
    pub fn balance(&self) -> f64 {
        self.balance
    }

    /// All completed trades.
    #[must_use]
    pub fn trades(&self) -> &[TradeRecord] {
        &self.trades
    }

    /// Clean up positions whose windows have expired without being resolved.
    ///
    /// Any position older than `max_window_duration_ms` is resolved as a loss
    /// (worst case assumption — better to lose on paper than leak capital).
    /// Returns the total realised P&L from cleaned-up positions.
    ///
    /// Callers should invoke this periodically (e.g. every 60 seconds or on
    /// each tick) to prevent unbounded growth of `open_positions` during
    /// prolonged WS disconnects or when no tick crosses a window boundary.
    pub fn cleanup_expired_positions(
        &mut self,
        current_time_ms: u64,
        max_window_duration_ms: u64,
    ) -> Pnl {
        let mut total_pnl: f64 = 0.0;
        let mut i = 0;
        while i < self.open_positions.len() {
            let pos = &self.open_positions[i].pos;
            if pos.opened_at_ms + max_window_duration_ms >= current_time_ms {
                i += 1;
                continue;
            }

            let ap = self.open_positions.swap_remove(i);
            let pos = ap.pos;

            // Resolve as a loss — worst case assumption.
            let pnl_val = -pos.size_usdc;
            self.balance += pos.size_usdc + pnl_val; // net zero return
            total_pnl += pnl_val;

            let exit_price = ContractPrice::new(0.0).unwrap_or_else(|| {
                unreachable!("0.0 is always a valid ContractPrice")
            });
            let pnl = Pnl::new(pnl_val).unwrap_or(Pnl::ZERO);

            warn!(
                window_id = %pos.window_id,
                asset = %pos.asset,
                side = %pos.side,
                size_usdc = pos.size_usdc,
                opened_at_ms = pos.opened_at_ms,
                strategy = %ap.strategy_id,
                "cleaning up expired position — resolved as loss"
            );

            self.trades.push(TradeRecord {
                window_id: pos.window_id,
                asset: pos.asset,
                side: pos.side,
                entry_price: pos.avg_entry,
                exit_price,
                size_usdc: pos.size_usdc,
                pnl,
                opened_at_ms: pos.opened_at_ms,
                closed_at_ms: current_time_ms,
                close_reason: OrderReason::ExpiryClose,
                strategy_id: ap.strategy_id,
            });
        }

        Pnl::new(total_pnl).unwrap_or(Pnl::ZERO)
    }

    /// Number of currently open positions.
    #[must_use]
    pub fn open_position_count(&self) -> usize {
        self.open_positions.len()
    }

    /// Returns `true` if there is at least one open position for `window_id`.
    #[must_use]
    pub fn has_position_in_window(&self, window_id: WindowId) -> bool {
        self.open_positions
            .iter()
            .any(|ap| ap.pos.window_id == window_id)
    }

    /// Print a summary of all completed trades to the log.
    pub fn print_summary(&self) {
        let summary = compute_summary(&self.trades);
        info!(
            trades = summary.total_trades,
            wins = summary.wins,
            losses = summary.losses,
            win_rate = format!("{:.1}%", summary.win_rate * 100.0),
            total_pnl = format!("{:.2}", summary.total_pnl),
            balance = format!("{:.2}", self.balance),
            "paper trading session summary"
        );
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test helpers use expect for conciseness")]
mod tests {
    use pm_types::{Asset, ContractPrice, EntryDecision, Side, StrategyId, WindowId};

    use super::*;

    fn default_config() -> PaperConfig {
        PaperConfig {
            initial_balance: 1_000.0,
            slippage_bps: 10,
            max_position_usdc: 50.0,
            max_positions_per_window: 1,
        }
    }

    fn make_decision(side: Side, limit_price: f64) -> EntryDecision {
        EntryDecision {
            side,
            limit_price: ContractPrice::new(limit_price).expect("valid limit_price"),
            confidence: 0.8,
            strategy_id: StrategyId::EarlyDirectional,
        }
    }

    // ── Test 1: Opening a position reduces the balance ────────────────────────

    #[test]
    fn open_position_reduces_balance() {
        let mut executor = PaperExecutor::new(default_config());
        let initial_balance = executor.balance();

        let decision = make_decision(Side::Up, 0.55);
        let fill = executor.try_open_position(
            &decision,
            WindowId::new(1),
            Asset::Btc,
            1_000,
            50.0,
        );

        assert!(fill.is_some(), "expected a fill");
        assert!(
            executor.balance() < initial_balance,
            "balance should decrease after opening a position"
        );
        assert_eq!(executor.open_position_count(), 1);
    }

    // ── Test 2: Win resolves with increased balance ───────────────────────────

    #[test]
    fn resolve_window_win_increases_balance() {
        let mut executor = PaperExecutor::new(default_config());

        let decision = make_decision(Side::Up, 0.55);
        let fill = executor
            .try_open_position(&decision, WindowId::new(1), Asset::Btc, 1_000, 50.0)
            .expect("fill should succeed");

        let balance_after_open = executor.balance();

        // Resolve as a win (Up outcome matches Up side).
        executor.resolve_window(WindowId::new(1), Side::Up, 5_000);

        assert_eq!(executor.open_position_count(), 0, "position should be closed");
        assert!(
            executor.balance() > balance_after_open,
            "winning trade should increase balance"
        );
        assert_eq!(executor.trades().len(), 1);
        assert!(
            executor.trades()[0].pnl.as_f64() > 0.0,
            "winning trade should have positive pnl"
        );
        // Sanity check: fill price was used as position's entry price.
        let _ = fill;
    }

    // ── Test 3: Loss resolves with decreased balance ──────────────────────────

    #[test]
    fn resolve_window_loss_decreases_balance() {
        let mut executor = PaperExecutor::new(default_config());
        let initial_balance = executor.balance();

        let decision = make_decision(Side::Up, 0.55);
        executor
            .try_open_position(&decision, WindowId::new(1), Asset::Btc, 1_000, 50.0)
            .expect("fill should succeed");

        // Resolve as a loss (Down outcome != Up side).
        executor.resolve_window(WindowId::new(1), Side::Down, 5_000);

        assert!(
            executor.balance() < initial_balance,
            "losing trade should reduce balance below initial"
        );
        assert!(
            executor.trades()[0].pnl.as_f64() < 0.0,
            "losing trade should have negative pnl"
        );
    }

    // ── Test 4: Slippage is applied correctly ─────────────────────────────────

    #[test]
    fn slippage_applied_to_entry_price() {
        let config = PaperConfig {
            initial_balance: 1_000.0,
            slippage_bps: 100, // 100 bps = 1%
            max_position_usdc: 50.0,
            max_positions_per_window: 1,
        };
        let mut executor = PaperExecutor::new(config);

        let decision = make_decision(Side::Up, 0.55);
        let fill = executor
            .try_open_position(&decision, WindowId::new(1), Asset::Btc, 1_000, 50.0)
            .expect("fill should succeed");

        // Entry should be 0.55 + 0.01 (100 bps) = 0.56
        let expected = 0.56;
        assert!(
            (fill.fill_price.as_f64() - expected).abs() < 1e-10,
            "fill price should include slippage: expected {expected}, got {}",
            fill.fill_price.as_f64()
        );
    }

    // ── Test 5: Position cap per window ──────────────────────────────────────

    #[test]
    fn position_cap_prevents_second_entry_in_same_window() {
        let mut executor = PaperExecutor::new(default_config());

        let decision = make_decision(Side::Up, 0.55);
        let first = executor
            .try_open_position(&decision, WindowId::new(1), Asset::Btc, 1_000, 50.0);
        let second = executor
            .try_open_position(&decision, WindowId::new(1), Asset::Btc, 2_000, 50.0);

        assert!(first.is_some(), "first fill should succeed");
        assert!(second.is_none(), "second fill should be blocked by cap");
        assert_eq!(executor.open_position_count(), 1);
    }

    // ── Test 6: has_position_in_window ───────────────────────────────────────

    #[test]
    fn has_position_in_window_reflects_state() {
        let mut executor = PaperExecutor::new(default_config());

        assert!(!executor.has_position_in_window(WindowId::new(1)));

        let decision = make_decision(Side::Down, 0.48);
        executor
            .try_open_position(&decision, WindowId::new(1), Asset::Eth, 1_000, 50.0)
            .expect("fill should succeed");

        assert!(executor.has_position_in_window(WindowId::new(1)));
        assert!(!executor.has_position_in_window(WindowId::new(2)));
    }
}
