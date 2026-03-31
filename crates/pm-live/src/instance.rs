//! Live execution wrapper around [`ConcreteStrategyInstance`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use pm_signal::ConcreteStrategyInstance;
use pm_types::{
    Asset, ContractPrice, FillEvent, InstanceStats, MarketState, Pnl, Side, StrategyId,
    StrategyInstance, Timeframe, TradeRecord, WindowId,
    trade::OrderReason,
};
use polymarket_client_sdk::clob::types::Side as ClobSide;
use tracing::{info, warn};

use crate::clob::{check_market_resolution, place_fok_order, place_gtc_order, ClobContext};

/// Maximum number of (asset, timeframe) slots.
const MAX_SLOTS: usize = Asset::COUNT * Timeframe::COUNT;

/// Token IDs for a market's Up and Down outcomes.
#[derive(Clone, Debug)]
pub struct TokenPair {
    /// Outcome token ID for the "Up" side.
    pub up: String,
    /// Outcome token ID for the "Down" side.
    pub down: String,
    /// Polymarket condition ID for resolution polling.
    pub condition_id: String,
    /// Window end time (Unix ms) for knowing when to start polling.
    pub end_date_ms: u64,
}

/// Shared token map: `(Asset, Timeframe)` -> `TokenPair`.
pub type SharedTokenMap = Arc<Mutex<HashMap<(Asset, Timeframe), TokenPair>>>;

/// A real-money position tracked by the live instance.
struct RealPosition {
    window_id: WindowId,
    asset: Asset,
    timeframe: Timeframe,
    side: Side,
    fill_price: f64,
    size_usdc: f64,
    shares: f64,
    #[expect(dead_code)]
    order_id: String,
    slot: usize,
    strategy_id: StrategyId,
    /// The token ID we bought (Up or Down outcome token).
    token_id: String,
    /// Polymarket condition ID for this market (used for resolution polling).
    condition_id: String,
    /// Expected window end time (Unix ms) — start polling after this.
    window_end_ms: u64,
}

/// Wraps a [`ConcreteStrategyInstance`] with real CLOB execution.
///
/// Signal evaluation, risk checks, and Kelly sizing are delegated to the inner
/// paper instance. When a signal fires, a real Fill-or-Kill market order is
/// placed via the Polymarket CLOB.
pub struct LiveStrategyInstance {
    /// Inner paper instance for signal evaluation and paper P&L comparison.
    paper: ConcreteStrategyInstance,

    /// CLOB client context (shared across live instances).
    clob: Arc<ClobContext>,

    /// Token ID mapping populated by the scanner.
    token_map: SharedTokenMap,

    /// Real balance tracking (starts equal to paper balance).
    real_balance: f64,
    /// Cumulative real P&L for the session.
    real_pnl: f64,
    /// Real session stats (wins, losses, etc.).
    real_stats: InstanceStats,

    /// Real open positions awaiting window resolution.
    real_positions: Vec<RealPosition>,

    /// Per-slot window dedup (same logic as paper instance).
    window_slots: [Option<WindowId>; MAX_SLOTS],

    /// Tokio runtime handle for blocking on async order placement.
    rt_handle: tokio::runtime::Handle,

    /// HTTP client for polling CLOB API market resolution.
    http_client: reqwest::Client,

    /// Order execution mode: "fok" or "gtc".
    order_mode: String,
    /// GTC timeout in milliseconds.
    gtc_timeout_ms: u64,
    /// Last GTC order placed, for the caller to drain and pass to OrderManager.
    last_gtc_order: Option<crate::order_manager::PendingOrder>,
    /// Optional channel for sending GTC orders to the main loop's OrderManager.
    gtc_order_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::order_manager::PendingOrder>>,
}

impl LiveStrategyInstance {
    /// Create a new live wrapper around a paper instance.
    ///
    /// # Panics
    ///
    /// Panics if called outside a tokio runtime context.
    #[must_use]
    pub fn new(
        paper: ConcreteStrategyInstance,
        clob: Arc<ClobContext>,
        token_map: SharedTokenMap,
        order_mode: String,
        gtc_timeout_secs: u64,
        gtc_order_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::order_manager::PendingOrder>>,
    ) -> Self {
        let balance = paper.balance();
        Self {
            paper,
            clob,
            token_map,
            real_balance: balance,
            real_pnl: 0.0,
            real_stats: InstanceStats::default(),
            real_positions: Vec::new(),
            window_slots: [None; MAX_SLOTS],
            rt_handle: tokio::runtime::Handle::current(),
            http_client: reqwest::Client::new(),
            order_mode,
            gtc_timeout_ms: gtc_timeout_secs * 1000,
            last_gtc_order: None,
            gtc_order_tx,
        }
    }

    /// Look up token ID, condition ID, and window end for (asset, timeframe, side).
    fn get_token_info(&self, asset: Asset, timeframe: Timeframe, side: Side) -> Option<(String, String, u64)> {
        let map = self.token_map.lock().ok()?;
        let pair = map.get(&(asset, timeframe))?;
        let token_id = match side {
            Side::Up => pair.up.clone(),
            Side::Down => pair.down.clone(),
        };
        Some((token_id, pair.condition_id.clone(), pair.end_date_ms))
    }

    /// Map our domain `Side` to the SDK's CLOB `Side`.
    fn to_clob_side(side: Side) -> ClobSide {
        // Buying an Up outcome token = Buy on the CLOB
        // Buying a Down outcome token = also Buy (different token_id)
        // We always BUY the outcome token we want.
        let _ = side;
        ClobSide::Buy
    }

    /// Resolve live positions using Polymarket's `market_resolved` event.
    ///
    /// `winning_token_id` is the token ID of the winning outcome, as reported
    /// by the Polymarket oracle. Positions holding this token win; others lose.
    ///
    /// Returns trade records for any positions that were resolved.
    pub fn on_market_resolved(
        &mut self,
        winning_token_id: &str,
        timestamp_ms: u64,
    ) -> Vec<TradeRecord> {
        let mut trades = Vec::new();
        let mut i = 0;
        while i < self.real_positions.len() {
            let pos = &self.real_positions[i];
            // Check if this position's token matches the winning token OR
            // the losing side of the same market (via token map).
            let is_this_market = {
                // Look up the token pair for this position's (asset, timeframe).
                let map = self.token_map.lock().ok();
                map.and_then(|m| {
                    let pair = m.get(&(pos.asset, pos.timeframe))?;
                    // This position is in this market if its token matches
                    // either the up or down token of the pair.
                    if pair.up == pos.token_id || pair.down == pos.token_id {
                        Some(())
                    } else {
                        // Token map may have rotated — check directly.
                        None
                    }
                })
                .is_some()
                    || pos.token_id == winning_token_id
            };

            if !is_this_market {
                i += 1;
                continue;
            }

            let pos = self.real_positions.swap_remove(i);
            let won = pos.token_id == winning_token_id;

            let pnl = if won {
                pos.shares - pos.size_usdc
            } else {
                -pos.size_usdc
            };

            self.real_balance += pos.size_usdc + pnl;
            self.real_pnl += pnl;
            self.real_stats.record(pnl);
            self.window_slots[pos.slot] = None;

            let exit_price_val = if won { 1.0 } else { 0.0 };

            #[expect(clippy::expect_used)]
            let fallback = ContractPrice::new(0.5).expect("0.5 is valid");

            let result_str = if won { "WIN" } else { "LOSS" };
            info!(
                instance = %self.label(),
                asset = %pos.asset,
                timeframe = ?pos.timeframe,
                side = %pos.side,
                result = result_str,
                pnl = format!("${pnl:+.2}"),
                balance = format!("${:.2}", self.real_balance),
                "LIVE trade resolved (on-chain)"
            );

            trades.push(TradeRecord {
                window_id: pos.window_id,
                asset: pos.asset,
                side: pos.side,
                entry_price: ContractPrice::new(pos.fill_price).unwrap_or(fallback),
                exit_price: ContractPrice::new(exit_price_val).unwrap_or(fallback),
                size_usdc: pos.size_usdc,
                pnl: Pnl::new(pnl).unwrap_or(Pnl::ZERO),
                opened_at_ms: 0,
                closed_at_ms: timestamp_ms,
                close_reason: OrderReason::ExpiryClose,
                strategy_id: pos.strategy_id,
            });
        }
        trades
    }

    /// Poll the CLOB API to resolve live positions whose windows have ended.
    ///
    /// For each position past `window_end_ms + 30s`, queries
    /// `GET /markets/{condition_id}` to check if the market resolved.
    /// When resolved, compares `winning_token_id` against our `token_id`
    /// to determine win/loss. Updates balance, P&L, and stats accordingly.
    ///
    /// Call this periodically (e.g. every 5-10 seconds) from the main loop.
    pub fn poll_resolutions(&mut self, http_client: &reqwest::Client, now_ms: u64) -> Vec<TradeRecord> {
        let mut trades = Vec::new();
        let mut i = 0;

        while i < self.real_positions.len() {
            let pos = &self.real_positions[i];

            // Only poll after window_end + 30 seconds.
            if now_ms < pos.window_end_ms + 30_000 {
                i += 1;
                continue;
            }

            let condition_id = pos.condition_id.clone();
            let result = tokio::task::block_in_place(|| {
                self.rt_handle.block_on(async {
                    check_market_resolution(http_client, &condition_id).await
                })
            });

            match result {
                Ok(res) if res.closed => {
                    let pos = self.real_positions.swap_remove(i);
                    let won = pos.token_id == res.winning_token_id;

                    let pnl = if won {
                        pos.shares - pos.size_usdc
                    } else {
                        -pos.size_usdc
                    };

                    self.real_balance += pos.size_usdc + pnl;
                    self.real_pnl += pnl;
                    self.real_stats.record(pnl);
                    self.window_slots[pos.slot] = None;

                    let result_str = if won { "WIN" } else { "LOSS" };
                    info!(
                        instance = %self.label(),
                        asset = %pos.asset,
                        side = %pos.side,
                        result = result_str,
                        pnl = format!("${pnl:+.2}"),
                        balance = format!("${:.2}", self.real_balance),
                        condition_id = %condition_id,
                        winning_token = %res.winning_token_id,
                        our_token = %pos.token_id,
                        "LIVE RESOLVED (oracle)"
                    );

                    let exit_price_val = if won { 1.0 } else { 0.0 };
                    #[expect(clippy::expect_used)]
                    let fallback = ContractPrice::new(0.5).expect("0.5 is valid");

                    trades.push(TradeRecord {
                        window_id: pos.window_id,
                        asset: pos.asset,
                        side: pos.side,
                        entry_price: ContractPrice::new(pos.fill_price).unwrap_or(fallback),
                        exit_price: ContractPrice::new(exit_price_val).unwrap_or(fallback),
                        size_usdc: pos.size_usdc,
                        pnl: Pnl::new(pnl).unwrap_or(Pnl::ZERO),
                        opened_at_ms: 0,
                        closed_at_ms: now_ms,
                        close_reason: OrderReason::ExpiryClose,
                        strategy_id: pos.strategy_id,
                    });
                    // Don't increment i — swap_remove moved last element here.
                }
                Ok(_) => {
                    // Not closed yet — try again next poll.
                    i += 1;
                }
                Err(e) => {
                    warn!(
                        instance = %self.label(),
                        condition_id = %condition_id,
                        error = %e,
                        "failed to poll market resolution"
                    );
                    i += 1;
                }
            }
        }

        trades
    }

    /// Number of open real positions (for monitoring).
    pub fn open_position_count(&self) -> usize {
        self.real_positions.len()
    }

    /// Drain the last GTC order placed for the OrderManager to track.
    pub fn take_pending_gtc(&mut self) -> Option<crate::order_manager::PendingOrder> {
        self.last_gtc_order.take()
    }

    /// Promote a filled GTC order into a RealPosition.
    pub fn promote_gtc_fill(&mut self, fill: &crate::order_manager::FilledOrder) {
        self.real_balance -= fill.size_usdc;
        self.real_positions.push(RealPosition {
            window_id: fill.window_id,
            asset: fill.asset,
            timeframe: fill.timeframe,
            side: fill.side,
            fill_price: fill.avg_price,
            size_usdc: fill.size_usdc,
            shares: fill.shares,
            order_id: fill.order_id.clone(),
            slot: fill.slot,
            strategy_id: fill.strategy_id,
            token_id: fill.token_id.clone(),
            condition_id: String::new(), // will be set from token map
            window_end_ms: 0,            // will be set from token map
        });

        info!(
            instance = %self.label(),
            asset = %fill.asset,
            side = %fill.side,
            fill_price = fill.avg_price,
            shares = fill.shares,
            size_usdc = fill.size_usdc,
            balance = self.real_balance,
            "GTC FILL PROMOTED"
        );
    }
}

impl StrategyInstance for LiveStrategyInstance {
    fn label(&self) -> &str {
        self.paper.label()
    }

    fn on_tick(&mut self, state: &MarketState) -> Option<FillEvent> {
        // 1. Slot check -- one position per (asset, timeframe, window).
        let slot = state.asset.index() * Timeframe::COUNT + state.timeframe.index();
        if let Some(wid) = self.window_slots[slot]
            && wid == state.window_id
        {
            return None;
        }

        // 2. Kill switch on daily loss.
        if self.real_pnl < -self.paper.max_daily_loss() {
            return None;
        }

        // 3. Evaluate signal (pure, no side effects).
        let decision = self.paper.evaluate_signal(state)?;

        // 4. Exposure check.
        let exposure: f64 = self.real_positions.iter().map(|p| p.size_usdc).sum();
        if exposure >= self.paper.max_exposure_usdc() {
            return None;
        }

        // 5. Kelly sizing on real balance.
        let size = (self.paper.kelly_fraction() * decision.confidence * self.real_balance)
            .min(self.paper.max_position_usdc())
            .min(self.real_balance * 0.05);
        // Polymarket CLOB enforces a $1 minimum order size.
        if size < 1.0 {
            return None;
        }

        // 6. Hard price guards — never buy outside [0.15, 0.85].
        //    Below 0.15: shares are near-worthless, market already decided.
        //    Above 0.85: terrible risk/reward, likely stale WS data.
        //    Also check the MarketState's live ask as a second guard.
        const MIN_LIVE_ENTRY: f64 = 0.15;
        const MAX_LIVE_ENTRY: f64 = 0.85;
        let ask = decision.limit_price.as_f64();
        let live_ask = state.direction_ask().map(|p| p.as_f64()).unwrap_or(0.0);
        if ask < MIN_LIVE_ENTRY || ask > MAX_LIVE_ENTRY
            || live_ask < MIN_LIVE_ENTRY || live_ask > MAX_LIVE_ENTRY {
            warn!(
                instance = %self.label(),
                asset = %state.asset,
                side = %decision.side,
                ask = ask,
                "LIVE PRICE GUARD — entry price outside [{MIN_LIVE_ENTRY}, {MAX_LIVE_ENTRY}], skipping"
            );
            self.window_slots[slot] = Some(state.window_id);
            return None;
        }

        // 7. Resolve token ID + condition ID from scanner-populated map.
        let (token_id, condition_id, window_end_ms) =
            self.get_token_info(state.asset, state.timeframe, decision.side)?;

        // 7b. GTC dispatch: place maker limit order, return None (fills arrive via WS).
        if self.order_mode == "gtc" {
            let clob = self.clob.clone();
            // Post 1 tick ($0.01) below the ask to rest on the book.
            // This avoids post_only rejection ("order crosses book").
            let ask_price = decision.limit_price.as_f64().min(MAX_LIVE_ENTRY);
            let price_rounded = ((ask_price - 0.01) * 100.0).floor() / 100.0;
            if price_rounded < MIN_LIVE_ENTRY {
                self.window_slots[slot] = Some(state.window_id);
                return None;
            }
            let size_shares = ((size / price_rounded) * 100.0).floor() / 100.0;

            // GTC orders have a minimum size of 5 shares on Polymarket.
            let size_shares = size_shares.max(5.0);

            let gtc_result = tokio::task::block_in_place(|| {
                self.rt_handle.block_on(async {
                    place_gtc_order(
                        &clob,
                        &token_id,
                        Self::to_clob_side(decision.side),
                        size_shares,
                        price_rounded,
                    )
                    .await
                })
            });

            match gtc_result {
                Ok(result) => {
                    info!(
                        instance = %self.label(),
                        order_id = %result.order_id,
                        asset = %state.asset,
                        side = %decision.side,
                        price = price_rounded,
                        size_shares = size_shares,
                        "GTC ORDER POSTED"
                    );
                    self.window_slots[slot] = Some(state.window_id);

                    // Build the pending order for OrderManager tracking.
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_millis() as u64);

                    let pending = crate::order_manager::PendingOrder {
                        order_id: result.order_id,
                        token_id,
                        asset: state.asset,
                        timeframe: state.timeframe,
                        side: decision.side,
                        price: price_rounded,
                        original_size_shares: size_shares,
                        filled_shares: 0.0,
                        filled_usdc: 0.0,
                        placed_at_ms: now_ms,
                        timeout_ms: self.gtc_timeout_ms,
                        strategy_id: decision.strategy_id,
                        slot,
                        window_id: state.window_id,
                        instance_label: self.label().to_string(),
                        condition_id: condition_id.clone(),
                        window_end_ms,
                    };

                    // Send via channel if available; otherwise store locally.
                    if let Some(ref tx) = self.gtc_order_tx {
                        let _ = tx.send(pending);
                    } else {
                        self.last_gtc_order = Some(pending);
                    }

                    // Also track in paper for comparison.
                    let _ = self.paper.on_tick(state);
                }
                Err(e) => {
                    warn!(
                        instance = %self.label(),
                        error = %e,
                        asset = %state.asset,
                        side = %decision.side,
                        "GTC ORDER FAILED"
                    );
                    self.window_slots[slot] = Some(state.window_id);
                }
            }
            return None; // GTC fills arrive asynchronously
        }

        // 8. Place market FOK order with slippage protection.
        let clob = self.clob.clone();
        let clob_side = Self::to_clob_side(decision.side);
        // Round to 2 decimal places — CLOB requires max 2dp for maker, 4dp for taker.
        let size_rounded = (size * 100.0).floor() / 100.0;
        let max_price = ((decision.limit_price.as_f64().min(MAX_LIVE_ENTRY)) * 100.0).floor() / 100.0;
        let fill_result = tokio::task::block_in_place(|| {
            self.rt_handle.block_on(async {
                place_fok_order(&clob, &token_id, clob_side, size_rounded, max_price).await
            })
        });

        match fill_result {
            Ok(fill) => {
                // Post-fill sanity: if the actual fill price is below our
                // minimum, we bought near-worthless shares on a stale/wrong
                // market. Log a critical warning (can't undo the fill).
                if fill.avg_price < MIN_LIVE_ENTRY && fill.avg_price > 0.0 {
                    warn!(
                        instance = %self.label(),
                        asset = %state.asset,
                        side = %decision.side,
                        fill_price = fill.avg_price,
                        size_usdc = fill.cost_usdc,
                        "BAD FILL — price below minimum, likely stale/wrong market"
                    );
                }

                self.real_balance -= fill.cost_usdc;
                self.real_positions.push(RealPosition {
                    window_id: state.window_id,
                    asset: state.asset,
                    timeframe: state.timeframe,
                    side: decision.side,
                    fill_price: fill.avg_price,
                    size_usdc: fill.cost_usdc,
                    shares: fill.shares,
                    order_id: fill.order_id,
                    slot,
                    strategy_id: decision.strategy_id,
                    token_id: token_id.clone(),
                    condition_id: condition_id.clone(),
                    window_end_ms,
                });
                self.window_slots[slot] = Some(state.window_id);

                // Also track in paper for comparison.
                let _ = self.paper.on_tick(state);

                info!(
                    instance = %self.label(),
                    asset = %state.asset,
                    side = %decision.side,
                    fill_price = fill.avg_price,
                    size_usdc = fill.cost_usdc,
                    balance = self.real_balance,
                    "LIVE FILL"
                );

                Some(FillEvent {
                    label: decision.label,
                    asset: state.asset,
                    timeframe: state.timeframe,
                    side: decision.side,
                    strategy_id: decision.strategy_id,
                    fill_price: fill.avg_price,
                    size_usdc: fill.cost_usdc,
                    confidence: decision.confidence,
                    balance_after: self.real_balance,
                })
            }
            Err(e) => {
                // Log full error chain for debugging
                let mut chain = String::new();
                let mut source = e.source();
                while let Some(cause) = source {
                    chain.push_str(&format!(" → {cause}"));
                    source = std::error::Error::source(cause);
                }
                warn!(
                    instance = %self.label(),
                    error = %e,
                    chain = %chain,
                    asset = %state.asset,
                    side = %decision.side,
                    size_usdc = size,
                    token_id = %token_id,
                    "LIVE ORDER FAILED"
                );
                // Mark slot so we don't spam retries on the same window
                self.window_slots[slot] = Some(state.window_id);
                None
            }
        }
    }

    fn on_window_close(
        &mut self,
        window_id: WindowId,
        outcome: Side,
        timestamp_ms: u64,
    ) -> Vec<TradeRecord> {
        // For live positions: do NOT resolve here. The internal spot-based
        // outcome is unreliable (window ID mismatch with actual PM market).
        // Instead, positions are resolved by poll_resolutions() which checks
        // the actual CLOB API for the oracle result.
        //
        // We only close paper positions for comparison stats.
        let _ = self.paper.on_window_close(window_id, outcome, timestamp_ms);

        Vec::new()
    }

    fn on_market_resolved(
        &mut self,
        winning_token_id: &str,
        timestamp_ms: u64,
    ) -> Vec<TradeRecord> {
        self.on_market_resolved(winning_token_id, timestamp_ms)
    }

    fn poll_resolutions(&mut self, now_ms: u64) -> Vec<TradeRecord> {
        self.poll_resolutions(&self.http_client.clone(), now_ms)
    }

    fn promote_gtc_fill(
        &mut self,
        order_id: &str,
        token_id: &str,
        condition_id: &str,
        window_end_ms: u64,
        asset: Asset,
        timeframe: Timeframe,
        side: Side,
        avg_price: f64,
        size_usdc: f64,
        shares: f64,
        slot: usize,
        window_id: WindowId,
        strategy_id: StrategyId,
    ) {
        self.real_balance -= size_usdc;
        self.real_positions.push(RealPosition {
            window_id,
            asset,
            timeframe,
            side,
            fill_price: avg_price,
            size_usdc,
            shares,
            order_id: order_id.to_string(),
            slot,
            strategy_id,
            token_id: token_id.to_string(),
            condition_id: condition_id.to_string(),
            window_end_ms,
        });

        info!(
            instance = %self.label(),
            asset = %asset,
            side = %side,
            fill_price = avg_price,
            shares = shares,
            size_usdc = size_usdc,
            balance = self.real_balance,
            "GTC FILL → position tracked for oracle resolution"
        );
    }

    fn balance(&self) -> f64 {
        self.real_balance
    }

    fn stats(&self) -> &InstanceStats {
        &self.real_stats
    }
}
