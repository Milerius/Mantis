//! GTC limit order tracking, fill accounting, and timeout enforcement.
//!
//! [`OrderManager`] maintains a list of resting GTC orders, applies fill
//! updates received from the User WebSocket, cancels orders that exceed their
//! placement timeout, and drains fully-filled or partially-filled-then-timed-
//! out orders as [`FilledOrder`] records for downstream PnL accounting.

use std::sync::Arc;

use pm_types::{Asset, Side, StrategyId, Timeframe, WindowId};
use tracing::{info, warn};

use crate::clob::{cancel_order, ClobContext};

// ─── PendingOrder ────────────────────────────────────────────────────────────

/// A GTC limit order that is currently resting on the Polymarket book.
#[derive(Debug, Clone)]
pub struct PendingOrder {
    /// Polymarket order ID returned by the CLOB on placement.
    pub order_id: String,
    /// Hex token ID for the outcome token.
    pub token_id: String,
    /// Underlying asset this market tracks.
    pub asset: Asset,
    /// Prediction window timeframe.
    pub timeframe: Timeframe,
    /// Direction of the bet (Up / Down).
    pub side: Side,
    /// Posted limit price per share (0.0–1.0).
    pub price: f64,
    /// Total shares requested when the order was placed.
    pub original_size_shares: f64,
    /// Cumulative shares matched so far.
    pub filled_shares: f64,
    /// Cumulative USDC value of matched shares (at maker posted price).
    pub filled_usdc: f64,
    /// Unix timestamp (ms) when the order was placed.
    pub placed_at_ms: u64,
    /// Maximum duration (ms) to leave the order resting before cancelling.
    pub timeout_ms: u64,
    /// Strategy that generated this order.
    pub strategy_id: StrategyId,
    /// Strategy instance slot index (for deduplication checks).
    pub slot: usize,
    /// Market window this order belongs to.
    pub window_id: WindowId,
    /// Human-readable label for the strategy instance.
    pub instance_label: String,
    /// Polymarket condition ID for resolution polling.
    pub condition_id: String,
    /// Expected window end time (Unix ms).
    pub window_end_ms: u64,
}

// ─── FilledOrder ─────────────────────────────────────────────────────────────

/// A completed order that has been drained from the pending list.
///
/// Represents either a fully-filled GTC order or a partially-filled order
/// that was subsequently cancelled due to timeout. Zero-fill timeouts are
/// discarded silently and never produce a [`FilledOrder`].
#[derive(Debug, Clone)]
pub struct FilledOrder {
    /// Polymarket order ID.
    pub order_id: String,
    /// Hex token ID for the outcome token.
    pub token_id: String,
    /// Underlying asset this market tracks.
    pub asset: Asset,
    /// Prediction window timeframe.
    pub timeframe: Timeframe,
    /// Direction of the bet (Up / Down).
    pub side: Side,
    /// Volume-weighted average fill price (`filled_usdc / shares`).
    pub avg_price: f64,
    /// Total USDC spent on this fill.
    pub size_usdc: f64,
    /// Total shares received.
    pub shares: f64,
    /// Strategy instance slot index.
    pub slot: usize,
    /// Market window this order belonged to.
    pub window_id: WindowId,
    /// Strategy that generated this order.
    pub strategy_id: StrategyId,
    /// Human-readable label for the strategy instance.
    pub instance_label: String,
    /// Polymarket condition ID for resolution polling.
    pub condition_id: String,
    /// Expected window end time (Unix ms).
    pub window_end_ms: u64,
}

// ─── OrderManager ────────────────────────────────────────────────────────────

/// Tracks resting GTC orders and drives their lifecycle.
///
/// # Usage
///
/// 1. Call [`OrderManager::track`] immediately after placing a GTC order.
/// 2. Feed WebSocket `order_fill_update` events into [`OrderManager::on_fill_update`].
/// 3. Feed WebSocket `order_cancel` events into [`OrderManager::on_cancel_event`].
/// 4. Call [`OrderManager::check_timeouts`] on a regular tick to cancel stale orders.
/// 5. Call [`OrderManager::drain_completed`] to harvest filled orders for PnL accounting.
pub struct OrderManager {
    /// All currently resting orders.
    pending: Vec<PendingOrder>,
    /// Authenticated CLOB client used to issue cancel requests.
    clob: Arc<ClobContext>,
    /// Tokio runtime handle used to block on async cancel calls from sync context.
    rt_handle: tokio::runtime::Handle,
}

impl OrderManager {
    /// Create a new [`OrderManager`] with an empty pending list.
    ///
    /// Captures the current Tokio runtime handle so that async cancel calls
    /// can be driven from synchronous code via [`tokio::task::block_in_place`].
    pub fn new(clob: Arc<ClobContext>) -> Self {
        Self {
            pending: Vec::new(),
            clob,
            rt_handle: tokio::runtime::Handle::current(),
        }
    }

    /// Begin tracking a newly placed GTC order.
    pub fn track(&mut self, order: PendingOrder) {
        info!(
            order_id = %order.order_id,
            token_id = %order.token_id,
            asset = %order.asset,
            timeframe = %order.timeframe,
            side = ?order.side,
            price = order.price,
            size_shares = order.original_size_shares,
            window_id = ?order.window_id,
            strategy_id = ?order.strategy_id,
            instance_label = %order.instance_label,
            "tracking GTC order",
        );
        self.pending.push(order);
    }

    /// Apply a partial or full fill event received from the User WebSocket.
    ///
    /// `size_matched` is the number of *additional* shares filled in this event
    /// (not a cumulative total). The USDC equivalent is computed at the order's
    /// posted maker price.
    pub fn on_fill_update(&mut self, order_id: &str, size_matched: f64) {
        if let Some(order) = self.pending.iter_mut().find(|o| o.order_id == order_id) {
            order.filled_shares += size_matched;
            order.filled_usdc += size_matched * order.price;
            info!(
                order_id = %order_id,
                size_matched,
                filled_shares = order.filled_shares,
                filled_usdc = order.filled_usdc,
                original_size_shares = order.original_size_shares,
                "fill update applied",
            );
        } else {
            warn!(order_id = %order_id, "fill update for unknown order — ignoring");
        }
    }

    /// Handle an external cancel notification from the User WebSocket.
    ///
    /// The order remains in the pending list; [`drain_completed`] will remove
    /// it on the next call (if it has any fill). This method only logs the
    /// event so that operators can observe external cancellations.
    ///
    /// [`drain_completed`]: OrderManager::drain_completed
    pub fn on_cancel_event(&mut self, order_id: &str) {
        if let Some(order) = self.pending.iter().find(|o| o.order_id == order_id) {
            info!(
                order_id = %order_id,
                filled_shares = order.filled_shares,
                "external cancel event received for order",
            );
        } else {
            warn!(order_id = %order_id, "cancel event for unknown order — ignoring");
        }
    }

    /// Cancel all orders whose placement timeout has elapsed.
    ///
    /// Iterates the pending list and, for each order where
    /// `now_ms >= placed_at_ms + timeout_ms`, issues a cancel request to the
    /// CLOB. The cancel is executed synchronously via
    /// [`tokio::task::block_in_place`] so this method can be called from a
    /// regular `&mut self` context inside an async task.
    ///
    /// Failed cancel attempts are logged as warnings but do not propagate
    /// errors — the order remains in the pending list and will be retried on
    /// the next tick.
    pub fn check_timeouts(&mut self, now_ms: u64) {
        for order in &self.pending {
            let deadline = order.placed_at_ms.saturating_add(order.timeout_ms);
            if now_ms < deadline {
                continue;
            }

            let order_id = order.order_id.clone();
            let clob = Arc::clone(&self.clob);
            let handle = self.rt_handle.clone();

            info!(
                order_id = %order_id,
                placed_at_ms = order.placed_at_ms,
                timeout_ms = order.timeout_ms,
                "order timeout reached — cancelling",
            );

            tokio::task::block_in_place(|| {
                if let Err(e) = handle.block_on(cancel_order(&*clob, &order_id)) {
                    warn!(order_id = %order_id, error = %e, "failed to cancel timed-out order");
                }
            });
        }
    }

    /// Cancel all pending orders that belong to the given window.
    ///
    /// Useful when a market window closes and any remaining resting orders
    /// must be pulled before settlement.
    pub fn cancel_all_for_window(&mut self, window_id: WindowId) {
        for order in self.pending.iter().filter(|o| o.window_id == window_id) {
            let order_id = order.order_id.clone();
            let clob = Arc::clone(&self.clob);
            let handle = self.rt_handle.clone();

            info!(
                order_id = %order_id,
                ?window_id,
                "cancelling order for window close",
            );

            tokio::task::block_in_place(|| {
                if let Err(e) = handle.block_on(cancel_order(&*clob, &order_id)) {
                    warn!(order_id = %order_id, error = %e, "failed to cancel order for window");
                }
            });
        }
    }

    /// Remove and return all completed orders as [`FilledOrder`] records.
    ///
    /// An order is considered *completed* when either:
    /// - It is fully filled: `filled_shares >= original_size_shares - 0.001`, or
    /// - Its timeout has elapsed and it has at least some fill.
    ///
    /// Timed-out orders with zero fill are silently discarded (no [`FilledOrder`]
    /// produced). Uses a `swap_remove`-based loop to avoid `O(n²)` shifting.
    pub fn drain_completed(&mut self, now_ms: u64) -> Vec<FilledOrder> {
        let mut completed = Vec::new();
        let mut i = 0;

        while i < self.pending.len() {
            let order = &self.pending[i];
            let timed_out = now_ms >= order.placed_at_ms.saturating_add(order.timeout_ms);
            let fully_filled = order.filled_shares >= order.original_size_shares - 0.001;

            if fully_filled || timed_out {
                let order = self.pending.swap_remove(i);
                // Only emit a FilledOrder when there is something to account for.
                if order.filled_shares > 0.0 {
                    let avg_price = order.filled_usdc / order.filled_shares;
                    completed.push(FilledOrder {
                        order_id: order.order_id,
                        token_id: order.token_id,
                        asset: order.asset,
                        timeframe: order.timeframe,
                        side: order.side,
                        avg_price,
                        size_usdc: order.filled_usdc,
                        shares: order.filled_shares,
                        slot: order.slot,
                        window_id: order.window_id,
                        strategy_id: order.strategy_id,
                        instance_label: order.instance_label,
                        condition_id: order.condition_id,
                        window_end_ms: order.window_end_ms,
                    });
                }
                // Do NOT increment i — the swapped element now occupies index i.
            } else {
                i += 1;
            }
        }

        completed
    }

    /// Returns `true` if there is at least one pending order for the given
    /// slot and window combination.
    ///
    /// Used by callers to avoid placing duplicate orders for the same
    /// strategy instance and window.
    pub fn has_pending_for_slot(&self, slot: usize, window_id: WindowId) -> bool {
        self.pending
            .iter()
            .any(|o| o.slot == slot && o.window_id == window_id)
    }

    /// Returns the total number of currently tracked pending orders.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}
