//! Paper trading subcommand: live WebSocket feeds + strategy evaluation + simulated fills.
//!
//! Wires together:
//! - [`BinanceWs`] / [`OkxWs`] — spot price tick streams
//! - [`OracleRouter`] — deduplicates ticks from multiple exchanges
//! - [`PriceBuffer`] — tracks per-asset open prices for window accounting
//! - [`MarketManager`] — discovers active Polymarket windows via Gamma API
//! - [`StrategyEngine`] — evaluates all strategies against live market state
//! - [`PaperExecutor`] — simulates fills with slippage
//! - [`RiskManager`] — enforces exposure/kill-switch rules before opening positions
//! - [`SnapshotRecorder`] — records combined spot + orderbook snapshots to plain JSONL

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context as _, Result};
use pm_bookkeeper::{SnapshotRecorder, WindowRecorder};
use pm_executor::{PaperConfig, PaperExecutor};
use pm_market::{
    L2OrderbookManager, LatestPrices, MarketManager, OrderbookTracker, PmEvent, PolymarketWs,
    SharedTokenAssetMap,
};
use pm_market::scanner::scan_active_markets;
use pm_oracle::{BinanceWs, EmaTracker, OkxWs, OracleRouter, PriceBuffer};
use pm_risk::{RiskConfig, RiskManager};
use pm_signal::{EntryTimer, PendingEntry, TrendFilter, build_engine_from_config};
use pm_types::{
    Asset, ContractPrice, MarketState, OpenPosition, Side, Timeframe, Tick, Window, WindowId,
    config::BotConfig,
};
use reqwest::Client;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

// ─── Constants ────────────────────────────────────────────────────────────────

/// Maximum window duration for expired-position cleanup (4 hours).
const MAX_WINDOW_DURATION_MS: u64 = 4 * 60 * 60 * 1_000;

/// Fallback ask price for the Up contract when no live orderbook is available.
const FALLBACK_ASK_UP: f64 = 0.55;

/// Fallback ask price for the Down contract when no live orderbook is available.
const FALLBACK_ASK_DOWN: f64 = 0.48;

/// Maximum age (in milliseconds) for cached prices before falling back.
/// If the PM WebSocket disconnects, prices older than this are considered stale.
const MAX_PRICE_AGE_MS: u64 = 15_000;

// ─── Module-level statics (atomic logging guards) ────────────────────────────

/// Guards a one-shot "first tick received" log line across loop iterations.
static FIRST_TICK_LOGGED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Monotonic counter of ticks processed; used for periodic throughput logging.
static TICK_COUNT: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

// ─── OrderbookPrices ──────────────────────────────────────────────────────────

/// Resolved orderbook prices for both legs of a binary market.
///
/// `rec_ask_*` / `rec_bid_*` fields are `Some` only when live PM WebSocket
/// prices were used; they are `None` when the fallback model was applied. The
/// snapshot recorder uses these `Option<f64>` values to distinguish live fills
/// from model fills.  The `contract_*` fields are `Option<ContractPrice>` as
/// required by [`MarketState`].
struct OrderbookPrices {
    /// Recorder-facing raw ask/bid values — `None` means fallback model used.
    rec_ask_up: Option<f64>,
    rec_ask_down: Option<f64>,
    rec_bid_up: Option<f64>,
    rec_bid_down: Option<f64>,
    /// MarketState-facing contract prices.
    contract_ask_up: Option<ContractPrice>,
    contract_ask_down: Option<ContractPrice>,
    contract_bid_up: Option<ContractPrice>,
    contract_bid_down: Option<ContractPrice>,
    /// L2 orderbook imbalance at top 5 levels, if available.
    orderbook_imbalance: Option<f64>,
}

// ─── Window tracking ─────────────────────────────────────────────────────────

/// Per-(asset, timeframe) window state updated on each tick.
struct LiveWindow {
    window: Window,
    /// Whether a position has already been opened in this window.
    position_opened: bool,
    /// Set after a signal has been evaluated (filled or rejected) to prevent
    /// re-evaluating the same window on every subsequent tick.
    signal_attempted: bool,
    /// Pending entry waiting for optimal execution conditions (smart entry timing).
    pending_entry: Option<PendingEntry>,
}

// ─── Session Stats ──────────────────────────────────────────────────────────

/// Tracks live session performance metrics.
struct SessionStats {
    start_time: std::time::Instant,
    initial_balance: f64,
    total_signals: u32,
    total_fills: u32,
    total_trend_filtered: u32,
    total_risk_rejected: u32,
    wins: u32,
    losses: u32,
    realized_pnl: f64,
    biggest_win: f64,
    biggest_loss: f64,
    wins_by_strategy: std::collections::HashMap<String, u32>,
    losses_by_strategy: std::collections::HashMap<String, u32>,
    last_summary_at: std::time::Instant,
}

impl SessionStats {
    fn new(initial_balance: f64) -> Self {
        let now = std::time::Instant::now();
        Self {
            start_time: now,
            initial_balance,
            total_signals: 0,
            total_fills: 0,
            total_trend_filtered: 0,
            total_risk_rejected: 0,
            wins: 0,
            losses: 0,
            realized_pnl: 0.0,
            biggest_win: 0.0,
            biggest_loss: 0.0,
            wins_by_strategy: std::collections::HashMap::new(),
            losses_by_strategy: std::collections::HashMap::new(),
            last_summary_at: now,
        }
    }

    fn record_resolution(&mut self, pnl: f64, strategy_name: &str) {
        if pnl >= 0.0 {
            self.wins += 1;
            *self.wins_by_strategy.entry(strategy_name.to_string()).or_insert(0) += 1;
            if pnl > self.biggest_win {
                self.biggest_win = pnl;
            }
        } else {
            self.losses += 1;
            *self.losses_by_strategy.entry(strategy_name.to_string()).or_insert(0) += 1;
            if pnl < self.biggest_loss {
                self.biggest_loss = pnl;
            }
        }
        self.realized_pnl += pnl;
    }

    fn win_rate(&self) -> f64 {
        let total = self.wins + self.losses;
        if total == 0 { 0.0 } else { self.wins as f64 / total as f64 * 100.0 }
    }

    fn log_summary(&mut self, current_balance: f64, open_positions: usize) {
        let uptime = self.start_time.elapsed();
        let mins = uptime.as_secs() / 60;
        let _total_closed = self.wins + self.losses;
        let roi = (current_balance - self.initial_balance) / self.initial_balance * 100.0;

        info!(
            uptime_min = mins,
            balance = format!("${:.2}", current_balance),
            roi = format!("{:+.1}%", roi),
            realized_pnl = format!("${:+.2}", self.realized_pnl),
            open_positions = open_positions,
            signals = self.total_signals,
            fills = self.total_fills,
            trend_filtered = self.total_trend_filtered,
            risk_rejected = self.total_risk_rejected,
            closed = format!("{}W/{}L ({:.0}%)", self.wins, self.losses, self.win_rate()),
            biggest_win = format!("${:.2}", self.biggest_win),
            biggest_loss = format!("${:.2}", self.biggest_loss),
            "📊 SESSION SUMMARY"
        );

        // Per-strategy breakdown
        let mut all_strats: std::collections::HashSet<String> = self.wins_by_strategy.keys().cloned().collect();
        all_strats.extend(self.losses_by_strategy.keys().cloned());
        for strat in &all_strats {
            let w = self.wins_by_strategy.get(strat).copied().unwrap_or(0);
            let l = self.losses_by_strategy.get(strat).copied().unwrap_or(0);
            let wr = if w + l > 0 { w as f64 / (w + l) as f64 * 100.0 } else { 0.0 };
            info!(
                strategy = %strat,
                record = format!("{}W/{}L ({:.0}%)", w, l, wr),
                "  strategy breakdown"
            );
        }

        self.last_summary_at = std::time::Instant::now();
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Fetch the best ask and bid prices for a single CLOB token from the REST
/// orderbook endpoint.
///
/// Pushes any valid prices into `tracker` so the PM WebSocket tracker has
/// initial state immediately — even on quiet markets where the WS won't fire
/// until the next book change.
async fn fetch_rest_orderbook(
    client: &Client,
    token_id: &str,
    tracker: &mut OrderbookTracker,
    now_ms: u64,
) {
    let url = format!("https://clob.polymarket.com/book?token_id={token_id}");
    match client.get(&url).send().await {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(book) => {
                if let Some(asks) = book.get("asks").and_then(|a| a.as_array())
                    && let Some(best) = asks.first() {
                        let price: f64 = best
                            .get("price")
                            .and_then(|p| p.as_str())
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0.0);
                        if price > 0.01 && price < 0.99 {
                            tracker.update(token_id, "SELL", price, now_ms);
                        }
                    }
                if let Some(bids) = book.get("bids").and_then(|a| a.as_array())
                    && let Some(best) = bids.first() {
                        let price: f64 = best
                            .get("price")
                            .and_then(|p| p.as_str())
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0.0);
                        if price > 0.01 && price < 0.99 {
                            tracker.update(token_id, "BUY", price, now_ms);
                        }
                    }
            }
            Err(e) => {
                warn!(
                    token_id = %token_id,
                    error = %e,
                    "failed to parse REST orderbook"
                );
            }
        },
        Err(e) => {
            warn!(
                token_id = %token_id,
                error = %e,
                "REST orderbook fetch failed"
            );
        }
    }
}

/// Build the fallback [`OrderbookPrices`] from model defaults when no live
/// orderbook data is available or when live prices look like a resolved market.
///
/// Called on every tick where the orderbook snapshot is `None` or the prices
/// are outside `(0.01, 0.99)`.
#[inline]
fn fallback_prices(spot_direction: Side, slippage: f64) -> OrderbookPrices {
    let base = if spot_direction == Side::Up {
        FALLBACK_ASK_UP
    } else {
        FALLBACK_ASK_DOWN
    };
    let opp = 1.0 - base + slippage;
    OrderbookPrices {
        rec_ask_up: None,
        rec_ask_down: None,
        rec_bid_up: None,
        rec_bid_down: None,
        contract_ask_up: ContractPrice::new(base.clamp(0.01, 0.99)),
        contract_ask_down: ContractPrice::new(opp.clamp(0.01, 0.99)),
        contract_bid_up: ContractPrice::new((base - 0.02).clamp(0.01, 0.99)),
        contract_bid_down: ContractPrice::new((opp - 0.02).clamp(0.01, 0.99)),
        orderbook_imbalance: None,
    }
}

/// Resolve orderbook prices for a market from the PM WebSocket tracker or fall
/// back to model defaults.
///
/// Called once per (asset, timeframe) per tick — before building [`MarketState`].
/// Reads from the local tracker and prices cache (no mutex locks).
/// Falls back to [`fallback_prices`] when no live data exists or
/// when prices appear to be from a resolved (settled) market.
fn resolve_orderbook_prices(
    tick: &Tick,
    timeframe: Timeframe,
    spot_direction: Side,
    slippage: f64,
    condition_id_opt: Option<&str>,
    local_tracker: &OrderbookTracker,
    local_prices: &LatestPrices,
    market_mgr: &MarketManager,
) -> OrderbookPrices {
    // PRIMARY: try the LatestPrices cache (indexed by Asset, Timeframe).
    // This is populated by PM WS events AND REST snapshots.
    // Only use the cache if BOTH sides have been populated by real events
    // to avoid trading against placeholder 0.50/0.48 prices.
    #[expect(clippy::cast_possible_truncation, reason = "millis since epoch fits in u64 for centuries")]
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    if let Some(p) = local_prices.get(tick.asset, timeframe) {
        // Staleness guard: if the cached price is older than the threshold,
        // skip it and fall through to the secondary source.
        if now_ms.saturating_sub(p.timestamp_ms) > MAX_PRICE_AGE_MS {
            warn!(
                asset = %tick.asset,
                timeframe = ?timeframe,
                age_ms = now_ms.saturating_sub(p.timestamp_ms),
                "cached price is stale — falling back to secondary source"
            );
        } else {
            let prices_are_sane =
                p.ask_up > 0.01 && p.ask_up < 0.99 && p.ask_down > 0.01 && p.ask_down < 0.99;
            if prices_are_sane && p.both_sides_seen() {
                return OrderbookPrices {
                    rec_ask_up: Some(p.ask_up),
                    rec_ask_down: Some(p.ask_down),
                    rec_bid_up: Some(p.bid_up),
                    rec_bid_down: Some(p.bid_down),
                    contract_ask_up: ContractPrice::new(p.ask_up),
                    contract_ask_down: ContractPrice::new(p.ask_down),
                    contract_bid_up: ContractPrice::new(p.bid_up),
                    contract_bid_down: ContractPrice::new(p.bid_down),
                    orderbook_imbalance: None,
                };
            }
        }
    }

    // SECONDARY: fall back to condition_id-based OrderbookTracker.
    let ob_snap = condition_id_opt.and_then(|cid| {
        if let Some(snap) = local_tracker.get(cid)
            && (snap.ask_up.is_some() || snap.ask_down.is_some())
        {
            return Some(*snap);
        }
        market_mgr.orderbook(cid).copied()
    });

    match ob_snap {
        Some(snap) if snap.ask_up.is_some() && snap.ask_down.is_some() => {
            let a_up = snap.ask_up.map_or(FALLBACK_ASK_UP, ContractPrice::as_f64);
            let a_down = snap.ask_down.map_or(FALLBACK_ASK_DOWN, ContractPrice::as_f64);
            let b_up = snap.bid_up.map_or(a_up - 0.02, ContractPrice::as_f64);
            let b_down = snap.bid_down.map_or(a_down - 0.02, ContractPrice::as_f64);

            // Sanity-check: prices from a resolved market sit at ~$0.00 or
            // ~$1.00 (fully settled). Reject anything outside (0.01, 0.99)
            // for both legs — those are useless for live trading and would
            // badly mis-price the model.
            let prices_are_sane =
                a_up > 0.01 && a_up < 0.99 && a_down > 0.01 && a_down < 0.99;

            if prices_are_sane {
                debug!(
                    asset = %tick.asset,
                    timeframe = ?timeframe,
                    ask_up = a_up,
                    ask_down = a_down,
                    "using live PM WS orderbook prices"
                );
                OrderbookPrices {
                    rec_ask_up: Some(a_up),
                    rec_ask_down: Some(a_down),
                    rec_bid_up: Some(b_up),
                    rec_bid_down: Some(b_down),
                    contract_ask_up: ContractPrice::new(a_up),
                    contract_ask_down: ContractPrice::new(a_down),
                    contract_bid_up: ContractPrice::new(b_up),
                    contract_bid_down: ContractPrice::new(b_down),
                    orderbook_imbalance: None,
                }
            } else {
                warn!(
                    asset = %tick.asset,
                    timeframe = ?timeframe,
                    ask_up = a_up,
                    ask_down = a_down,
                    "PM WS prices look like a resolved market — falling back to model defaults"
                );
                fallback_prices(spot_direction, slippage)
            }
        }
        _ => fallback_prices(spot_direction, slippage),
    }
}

/// Build a [`MarketState`] from a tick, the current window, and resolved prices.
///
/// Called once per (asset, timeframe) per tick after prices have been resolved.
#[inline]
fn build_market_state(
    tick: &Tick,
    timeframe: Timeframe,
    window: &Window,
    prices: &OrderbookPrices,
) -> MarketState {
    let magnitude = window.magnitude(tick.price);
    let time_elapsed_secs =
        (tick.timestamp_ms.saturating_sub(window.open_time_ms)) / 1_000;
    let time_remaining_secs = window.time_remaining_secs(tick.timestamp_ms);
    let spot_direction = window.direction(tick.price);

    MarketState {
        asset: tick.asset,
        timeframe,
        window_id: window.id,
        window_open_price: window.open_price,
        current_spot: tick.price,
        spot_magnitude: magnitude,
        spot_direction,
        time_elapsed_secs,
        time_remaining_secs,
        contract_ask_up: prices.contract_ask_up,
        contract_ask_down: prices.contract_ask_down,
        contract_bid_up: prices.contract_bid_up,
        contract_bid_down: prices.contract_bid_down,
        orderbook_imbalance: prices.orderbook_imbalance,
    }
}

/// Compute L2 orderbook imbalance for the Up token of a market.
///
/// Returns `None` if no L2 data is available for the token.
fn compute_l2_imbalance(
    l2_manager: &L2OrderbookManager,
    up_token_id: Option<&str>,
) -> Option<f64> {
    let token_id = up_token_id?;
    let book = l2_manager.get_book(token_id)?;
    Some(book.imbalance(5))
}

/// Format a [`WindowId`] into a stack-allocated buffer without heap allocation.
///
/// Writes `"W{inner}"` into `buf` (clearing it first) and returns `buf.as_str()`.
/// Use instead of `window_id.to_string()` in the hot path.
#[inline]
fn window_id_str<'b>(id: WindowId, buf: &'b mut String) -> &'b str {
    buf.clear();
    // WindowId::fmt writes "W{inner_u64}" — write! reuses the existing heap buffer.
    let _ = write!(buf, "{id}");
    buf.as_str()
}

/// Handle a window transition: resolve the old window and open a new one.
///
/// Called when a tick's timestamp has crossed the close boundary of the current
/// live window for a given (asset, timeframe) slot. Records the outcome via both
/// the [`WindowRecorder`] and the [`PaperExecutor`], then initialises a fresh
/// [`LiveWindow`] for the new period.
///
/// # Failures
///
/// All recorder failures are logged via `warn!` and do not abort the loop.
fn handle_window_transition(
    slot: usize,
    tick: &Tick,
    timeframe: Timeframe,
    window_open_ms: u64,
    window_close_ms: u64,
    live_windows: &mut Vec<Option<LiveWindow>>,
    executor: &mut PaperExecutor,
    risk: &mut RiskManager,
    window_recorder: &mut WindowRecorder,
    stats: &mut SessionStats,
) {
    if let Some(old_lw) = live_windows[slot].take() {
        let outcome = old_lw.window.direction(tick.price);

        // Resolve the window in the executor first so we get the actual
        // realised P&L for this window.
        let window_pnl =
            executor.resolve_window(old_lw.window.id, outcome, tick.timestamp_ms);

        // Notify risk manager with the real P&L so it can track cumulative
        // daily loss correctly.
        risk.on_window_resolved(old_lw.window.id, window_pnl);

        // Close the PBT-compatible window recording.
        // `as_lower_str` / `as_label` return &'static str — zero allocation.
        let mut win_buf = String::with_capacity(24);
        let win_id = window_id_str(old_lw.window.id, &mut win_buf);
        let old_key = WindowRecorder::window_key(
            old_lw.window.asset.as_lower_str(),
            old_lw.window.timeframe.as_label(),
            win_id,
        );
        let outcome_str = format!("{outcome}");
        if let Err(e) = window_recorder.close_window(&old_key, &outcome_str, tick.price.as_f64())
        {
            warn!(error = %e, key = %old_key, "failed to close window recording");
        }

        let pnl_val = window_pnl.as_f64();
        let had_position = old_lw.position_opened;

        if had_position {
            // Find what strategy opened this position (use "unknown" if not tracked)
            let strat_name = "mixed"; // TODO: track strategy per position
            stats.record_resolution(pnl_val, strat_name);

            let result = if pnl_val >= 0.0 { "WIN" } else { "LOSS" };
            info!(
                asset = %tick.asset,
                timeframe = ?timeframe,
                window_id = %old_lw.window.id,
                outcome = %outcome,
                result = result,
                pnl = format!("${:+.2}", pnl_val),
                balance = format!("${:.2}", executor.balance()),
                record = format!("{}W/{}L ({:.0}%)", stats.wins, stats.losses, stats.win_rate()),
                "window resolved — TRADE CLOSED"
            );
        } else {
            info!(
                asset = %tick.asset,
                timeframe = ?timeframe,
                window_id = %old_lw.window.id,
                outcome = %outcome,
                "window resolved — no position"
            );
        }
    }

    let raw_id = (window_open_ms / 1_000) ^ (tick.asset.index() as u64 * 0x9E37_79B9)
        ^ (timeframe.duration_secs() * 7);
    let new_window = Window {
        id: WindowId::new(raw_id),
        asset: tick.asset,
        timeframe,
        open_time_ms: window_open_ms,
        close_time_ms: window_close_ms,
        open_price: tick.price,
    };
    live_windows[slot] = Some(LiveWindow {
        window: new_window,
        position_opened: false,
                signal_attempted: false,
        pending_entry: None,
    });

    // Open a PBT-compatible window recording.
    // ISO timestamp conversion only happens on window open/close, not every tick.
    #[expect(clippy::cast_possible_wrap, reason = "timestamps are well within i64 range for millennia")]
    let start_iso = chrono::DateTime::from_timestamp_millis(window_open_ms as i64)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default();
    #[expect(clippy::cast_possible_wrap, reason = "timestamps are well within i64 range for millennia")]
    let end_iso = chrono::DateTime::from_timestamp_millis(window_close_ms as i64)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default();
    let mut win_buf = String::with_capacity(24);
    let win_id = window_id_str(new_window.id, &mut win_buf);
    window_recorder.open_window(
        tick.asset.as_lower_str(),
        timeframe.as_label(),
        win_id,
        &start_iso,
        &end_iso,
        tick.price.as_f64(),
    );

    info!(
        asset = %tick.asset,
        timeframe = ?timeframe,
        window_id = %new_window.id,
        open_price = tick.price.as_f64(),
        "new window opened"
    );
}

/// Process a single deduplicated tick across all enabled (asset, timeframe) slots.
///
/// Called on every tick that passes through the oracle router. For each enabled
/// timeframe this function:
/// 1. Transitions the live window if the current tick has crossed a window boundary.
/// 2. Skips slots where a position has already been opened.
/// 3. Resolves orderbook prices (live PM WS or model fallback).
/// 4. Records a combined spot + orderbook snapshot.
/// 5. Evaluates all strategies and, if any signal fires, attempts a paper fill.
///
/// `now_utc` must be computed **once per tick** before calling this function —
/// it is passed in to avoid redundant `Utc::now()` syscalls inside the
/// timeframe loop.
#[expect(clippy::too_many_arguments)]
fn process_tick(
    tick: &Tick,
    enabled_timeframes: &[Timeframe],
    asset_slot: usize,
    slippage: f64,
    now_utc: chrono::DateTime<chrono::Utc>,
    live_windows: &mut Vec<Option<LiveWindow>>,
    executor: &mut PaperExecutor,
    risk: &mut RiskManager,
    local_tracker: &OrderbookTracker,
    local_prices: &LatestPrices,
    market_mgr: &MarketManager,
    recorder: &mut SnapshotRecorder,
    window_recorder: &mut WindowRecorder,
    engine: &pm_signal::StrategyEngine,
    ema_tracker: &mut EmaTracker,
    trend_filter: &TrendFilter,
    local_l2: &L2OrderbookManager,
    entry_timer: Option<&EntryTimer>,
    stats: &mut SessionStats,
) {
    // Update the EMA tracker with the latest price for this asset.
    ema_tracker.update(tick.asset, tick.price.as_f64());

    for (tf_idx, &timeframe) in enabled_timeframes.iter().enumerate() {
        let slot = asset_slot * enabled_timeframes.len() + tf_idx;
        let duration_ms = timeframe.duration_secs() * 1_000;
        let window_open_ms = tick.timestamp_ms - (tick.timestamp_ms % duration_ms);
        let window_close_ms = window_open_ms + duration_ms;

        let need_new = live_windows[slot]
            .as_ref()
            .is_none_or(|lw| tick.timestamp_ms >= lw.window.close_time_ms);

        if need_new {
            handle_window_transition(
                slot,
                tick,
                timeframe,
                window_open_ms,
                window_close_ms,
                live_windows,
                executor,
                risk,
                window_recorder,
                stats,
            );
        }

        let Some(lw) = live_windows[slot].as_mut() else {
            continue;
        };

        if lw.position_opened || lw.signal_attempted {
            continue;
        }

        let window = lw.window;
        let magnitude = window.magnitude(tick.price);
        let time_elapsed_secs =
            (tick.timestamp_ms.saturating_sub(window.open_time_ms)) / 1_000;
        let spot_direction = window.direction(tick.price);

        if time_elapsed_secs % 30 == 0 && time_elapsed_secs > 0 {
            debug!(
                asset = %tick.asset,
                timeframe = ?timeframe,
                mag = format!("{:.4}%", magnitude * 100.0),
                elapsed = time_elapsed_secs,
                dir = %spot_direction,
                spot = tick.price.as_f64(),
                open = window.open_price.as_f64(),
                "tick sample"
            );
        }

        // Match only a market whose window has NOT yet ended so we never bind
        // to a recently-resolved market that still appears in the scanner list
        // with stale orderbook prices.  `now_utc` is computed once per tick
        // outside this loop to avoid repeated syscalls.
        let matched_ids = market_mgr
            .active_markets()
            .find(|m| {
                if m.asset != tick.asset || m.timeframe != timeframe {
                    return false;
                }
                // Accept only markets whose end_date is still in the future.
                if m.end_date.is_empty() {
                    return true; // unknown date — pass through
                }
                m.end_date
                    .parse::<chrono::DateTime<chrono::Utc>>()
                    .map_or(true, |end_dt| end_dt > now_utc)
            })
            .map(|m| (m.condition_id.clone(), m.token_id_up.clone(), m.liquidity, m.spread));
        let condition_id_opt = matched_ids.as_ref().map(|(cid, _, _, _)| cid.clone());
        let up_token_id_opt = matched_ids.as_ref().map(|(_, tid, _, _)| tid.as_str());
        let market_liquidity = matched_ids.as_ref().map(|(_, _, liq, _)| *liq).unwrap_or(0.0);
        let market_spread = matched_ids.as_ref().map(|(_, _, _, spr)| *spr).unwrap_or(0.0);

        let mut prices = resolve_orderbook_prices(
            tick,
            timeframe,
            spot_direction,
            slippage,
            condition_id_opt.as_deref(),
            local_tracker,
            local_prices,
            market_mgr,
        );

        // Compute L2 orderbook imbalance from the Up token's full-depth book.
        prices.orderbook_imbalance =
            compute_l2_imbalance(local_l2, up_token_id_opt);

        let state = build_market_state(tick, timeframe, &window, &prices);

        // Record combined snapshot after building MarketState.
        if let Err(e) = recorder.record(
            tick.timestamp_ms,
            &tick.asset.to_string(),
            tick.price.as_f64(),
            prices.rec_ask_up,
            prices.rec_ask_down,
            prices.rec_bid_up,
            prices.rec_bid_down,
            window_open_ms,
            timeframe.duration_secs(),
        ) {
            warn!(error = %e, "failed to record snapshot");
        }

        // Record to PBT-compatible per-window buffer.
        {
            let mut win_buf = String::with_capacity(24);
            let win_id = window_id_str(window.id, &mut win_buf);
            let wkey = WindowRecorder::window_key(
                tick.asset.as_lower_str(),
                timeframe.as_label(),
                win_id,
            );
            #[expect(clippy::cast_possible_wrap, reason = "timestamps are well within i64 range for millennia")]
            let snap_iso = chrono::DateTime::from_timestamp_millis(tick.timestamp_ms as i64)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default();
            if let Err(e) = window_recorder.record_snapshot(
                &wkey,
                &snap_iso,
                tick.price.as_f64(),
                prices.rec_ask_up,
                prices.rec_ask_down,
            ) {
                warn!(error = %e, "failed to record window snapshot");
            }
        }

        // ── Check pending entry (smart entry timing) ───────────────────
        // If there is already a pending entry for this window, check if it
        // should execute now before evaluating new strategies.
        if let Some(timer) = entry_timer
            && let Some(ref mut pending) = lw.pending_entry
        {
            // Compute current spread from prices.
            let current_ask = match pending.decision.side {
                Side::Up => prices.rec_ask_up,
                Side::Down => prices.rec_ask_down,
            };
            let current_spread = match (prices.rec_ask_up, prices.rec_bid_up) {
                (Some(ask), Some(bid)) => Some(ask - bid),
                _ => None,
            };

            // Update best price tracking.
            if let Some(ask) = current_ask {
                EntryTimer::update_best_price(pending, ask);
            }

            if timer.should_execute(pending, tick.timestamp_ms, current_ask, current_spread) {
                let decision = pending.decision;
                // Clear the pending entry before attempting execution.
                lw.pending_entry = None;

                // Run through risk + executor.
                let sized_order = match risk.evaluate(
                    &decision,
                    window.id,
                    tick.asset,
                    executor.balance(),
                ) {
                    Ok(order) => order,
                    Err(rejection) => {
                        warn!(
                            asset = %tick.asset,
                            side = %decision.side,
                            rejection = ?rejection,
                            "risk manager rejected pending entry"
                        );
                        continue;
                    }
                };

                if let Some(fill) = executor.try_open_position(
                    &decision,
                    window.id,
                    tick.asset,
                    tick.timestamp_ms,
                    sized_order.size_usdc,
                ) {
                    risk.on_position_opened(OpenPosition {
                        window_id: window.id,
                        asset: tick.asset,
                        side: decision.side,
                        avg_entry: fill.fill_price,
                        size_usdc: sized_order.size_usdc,
                        opened_at_ms: tick.timestamp_ms,
                    });
                    lw.position_opened = true;
                    info!(
                        asset = %tick.asset,
                        side = %decision.side,
                        fill_price = fill.fill_price.as_f64(),
                        size_usdc = fill.size_usdc,
                        balance = executor.balance(),
                        "pending entry executed (smart timing)"
                    );
                }
            }
            // If we have a pending entry (still waiting) or just executed, skip new signals.
            if lw.position_opened || lw.pending_entry.is_some() {
                continue;
            }
        }

        let decisions = engine.evaluate_all(&state);

        for decision in &decisions {
            stats.total_signals += 1;
            info!(
                asset = %tick.asset,
                timeframe = ?timeframe,
                side = %decision.side,
                strategy = %decision.strategy_id,
                variant = %decision.label,
                confidence = decision.confidence,
                limit_price = decision.limit_price.as_f64(),
                liquidity = format!("${:.0}", market_liquidity),
                spread = format!("{:.3}", market_spread),
                imbalance = format!("{:.2}", prices.orderbook_imbalance.unwrap_or(0.0)),
                "strategy signal fired"
            );

            // Check trend filter before passing to risk manager.
            let trend = ema_tracker.trend(tick.asset);
            let strength = ema_tracker.trend_strength(tick.asset);
            if trend_filter.should_skip(decision.side, trend, strength) {
                stats.total_trend_filtered += 1;
                info!(
                    asset = %tick.asset,
                    timeframe = ?timeframe,
                    side = %decision.side,
                    strategy = %decision.strategy_id,
                    trend = ?trend,
                    strength = strength,
                    "trend filter skipped trade"
                );
                continue;
            }

            // ── Smart entry timing gate ─────────────────────────────────
            // If entry timing is enabled, store the decision as a pending
            // entry instead of executing immediately. The pending entry will
            // be checked on subsequent ticks.
            if let Some(_timer) = entry_timer {
                let current_spread = match (prices.rec_ask_up, prices.rec_bid_up) {
                    (Some(ask), Some(bid)) => Some(ask - bid),
                    _ => None,
                };
                lw.pending_entry = Some(PendingEntry {
                    decision: *decision,
                    signal_time_ms: tick.timestamp_ms,
                    initial_spread: current_spread,
                    best_price_seen: decision.limit_price.as_f64(),
                });
                debug!(
                    asset = %tick.asset,
                    timeframe = ?timeframe,
                    side = %decision.side,
                    "entry timing: queued pending entry"
                );
                // Only queue one pending entry per window.
                break;
            }

            // Mark this window as attempted so we don't re-evaluate on the next tick.
            lw.signal_attempted = true;

            // Run decision through risk manager before opening.
            let sized_order = match risk.evaluate(
                decision,
                window.id,
                tick.asset,
                executor.balance(),
            ) {
                Ok(order) => order,
                Err(rejection) => {
                    stats.total_risk_rejected += 1;
                    warn!(
                        asset = %tick.asset,
                        side = %decision.side,
                        rejection = ?rejection,
                        "risk manager rejected entry"
                    );
                    break; // stop evaluating more decisions for this window
                }
            };

            if let Some(fill) = executor.try_open_position(
                decision,
                window.id,
                tick.asset,
                tick.timestamp_ms,
                sized_order.size_usdc,
            ) {
                // Notify risk manager so it tracks exposure.
                risk.on_position_opened(OpenPosition {
                    window_id: window.id,
                    asset: tick.asset,
                    side: decision.side,
                    avg_entry: fill.fill_price,
                    size_usdc: sized_order.size_usdc,
                    opened_at_ms: tick.timestamp_ms,
                });

                lw.position_opened = true;
                stats.total_fills += 1;
                info!(
                    asset = %tick.asset,
                    side = %decision.side,
                    strategy = %decision.strategy_id,
                    variant = %decision.label,
                    fill_price = fill.fill_price.as_f64(),
                    size_usdc = fill.size_usdc,
                    balance = format!("${:.2}", executor.balance()),
                    record = format!("{}W/{}L", stats.wins, stats.losses),
                    liquidity = format!("${:.0}", market_liquidity),
                    spread = format!("{:.3}", market_spread),
                    "paper fill executed"
                );
                // Only one position per window.
                break;
            }
        }
    }
}

// ─── run_paper ────────────────────────────────────────────────────────────────

/// Run the paper trading loop.
///
/// Connects to Binance and OKX `WebSockets`, polls the Gamma API for active
/// markets, evaluates strategies on every tick, and simulates fills via the
/// [`PaperExecutor`]. Runs until SIGINT is received.
///
/// # Errors
///
/// Returns an error if the initial scanner poll fails or if a critical I/O
/// error occurs during startup.
pub async fn run_paper(cfg: &BotConfig) -> Result<()> {
    // ── 1. Collect enabled assets and timeframes ──────────────────────────────
    let enabled_assets: Vec<Asset> = cfg
        .bot
        .assets
        .iter()
        .filter(|a| a.enabled)
        .map(|a| a.asset)
        .collect();

    if enabled_assets.is_empty() {
        warn!("no enabled assets in config — paper trading has nothing to do");
        return Ok(());
    }

    let enabled_timeframes: Vec<Timeframe> = cfg
        .bot
        .assets
        .iter()
        .filter(|a| a.enabled)
        .flat_map(|a| a.timeframes.iter().copied())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    info!(
        assets = ?enabled_assets,
        timeframes = ?enabled_timeframes,
        "starting paper trading loop"
    );

    // ── 2. Channels and cancellation token ────────────────────────────────────
    let (tick_tx, mut tick_rx) = broadcast::channel::<Tick>(4096);
    let shutdown = CancellationToken::new();

    // ── 3. Spawn oracle WebSocket tasks ───────────────────────────────────────
    let binance = BinanceWs::new(enabled_assets.clone());
    let okx = OkxWs::new(enabled_assets.clone());

    let binance_tx = tick_tx.clone();
    let binance_shutdown = shutdown.clone();
    tokio::spawn(async move {
        if let Err(e) = binance.run(binance_tx, binance_shutdown).await {
            warn!(error = %e, "Binance WS task exited with error");
        }
    });

    let okx_tx = tick_tx.clone();
    let okx_shutdown = shutdown.clone();
    tokio::spawn(async move {
        if let Err(e) = okx.run(okx_tx, okx_shutdown).await {
            warn!(error = %e, "OKX WS task exited with error");
        }
    });

    // ── 4. Initialise components ──────────────────────────────────────────────
    let paper_config = PaperConfig {
        initial_balance: cfg.backtest.initial_balance,
        slippage_bps: cfg.backtest.slippage_bps,
        max_position_usdc: cfg.bot.max_position_usdc,
        max_positions_per_window: 1,
    };
    let mut executor = PaperExecutor::new(paper_config);

    let risk_config = RiskConfig {
        max_position_usdc: cfg.bot.max_position_usdc,
        max_total_exposure_usdc: cfg.bot.max_total_exposure_usdc,
        max_daily_loss_usdc: cfg.bot.max_daily_loss_usdc,
        kelly_fraction: cfg.bot.kelly_fraction,
        max_same_side_positions: 4,
    };
    let mut risk = RiskManager::new(risk_config);

    let engine = build_engine_from_config(&cfg.bot.strategies);

    let mut oracle_router = OracleRouter::new();
    let mut price_buffer = PriceBuffer::new();

    // ── Session stats ────────────────────────────────────────────────────────
    let mut stats = SessionStats::new(cfg.backtest.initial_balance);

    // ── EMA-based trend filter ──────────────────────────────────────────────
    let tf_cfg = &cfg.bot.trend_filter;
    let mut ema_tracker = EmaTracker::new(tf_cfg.fast_period, tf_cfg.slow_period);
    let trend_filter = TrendFilter {
        require_trend_alignment: tf_cfg.enabled,
        min_trend_strength: tf_cfg.min_trend_strength,
    };

    // ── Smart entry timing ─────────────────────────────────────────────────
    let et_cfg = &cfg.bot.entry_timing;
    let entry_timer_opt = if et_cfg.enabled {
        Some(EntryTimer::new(et_cfg.max_wait_secs, et_cfg.min_spread_improvement))
    } else {
        None
    };

    // Per-(asset, timeframe) window table.
    let num_slots = enabled_assets.len() * enabled_timeframes.len();
    let mut live_windows: Vec<Option<LiveWindow>> = (0..num_slots).map(|_| None).collect();

    // ── Local orderbook state (owned by the main loop — no Arc<Mutex>) ────────
    let mut local_tracker = OrderbookTracker::new();
    let mut local_prices = LatestPrices::new();
    let mut local_l2 = L2OrderbookManager::new();

    // Shared token → (Asset, Timeframe, is_up) map, populated after each scan.
    // Kept as Arc<Mutex> because it is written by the scanner and read by the
    // main loop's event handler (different directions, low frequency).
    let token_asset_map: SharedTokenAssetMap =
        Arc::new(Mutex::new(HashMap::new()));

    let mut market_mgr = MarketManager::new(Duration::from_secs(cfg.bot.scan_interval_secs));
    let http_client = Client::new();
    let mut next_scan_at = tokio::time::Instant::now();

    let mut subscribed_tokens: HashSet<String> = HashSet::new();

    // ── Event channel: WS task → main loop ───────────────────────────────────
    let (pm_event_tx, mut pm_event_rx) = tokio::sync::mpsc::unbounded_channel::<PmEvent>();

    let (pm_ws, pm_new_tokens_tx, pm_needs_refresh) = PolymarketWs::new_with_events(
        Vec::new(),
        Arc::clone(&token_asset_map),
        pm_event_tx,
    );
    let pm_shutdown = shutdown.clone();
    tokio::spawn(async move {
        // The tracker Arc<Mutex> is only needed for the run() API signature;
        // the WS task sends events through the channel instead of locking it.
        let dummy_tracker = Arc::new(Mutex::new(OrderbookTracker::new()));
        pm_ws.run(dummy_tracker, pm_shutdown).await;
    });

    // ── Snapshot recorder (plain JSONL, flush every 10 writes) ────────────────
    let data_dir = Path::new(&cfg.data.cache_dir);
    let session_id = chrono::Utc::now().format("%Y%m%dT%H%M%S").to_string();
    let mut recorder = SnapshotRecorder::new(data_dir, &session_id)
        .context("failed to create snapshot recorder")?;

    info!(
        session_id = %session_id,
        path = %data_dir.join("live").join(format!("{session_id}_snapshots.jsonl")).display(),
        "snapshot recorder started"
    );

    // ── Per-window PBT-compatible recorder ───────────────────────────────────
    let mut window_recorder = WindowRecorder::new(data_dir)
        .context("failed to create window recorder")?;
    info!("PBT-compatible window recorder started");

    // ── 5. Graceful shutdown signal ───────────────────────────────────────────
    let ctrlc_shutdown = shutdown.clone();
    tokio::spawn(async move {
        if let Err(e) = tokio::signal::ctrl_c().await {
            warn!(error = %e, "ctrl-c listener error");
        }
        info!("SIGINT received — shutting down");
        ctrlc_shutdown.cancel();
    });

    // ── 6. Main event loop ────────────────────────────────────────────────────
    let slippage = f64::from(cfg.backtest.slippage_bps) * 0.0001;

    let mut next_cleanup_at = tokio::time::Instant::now() + Duration::from_secs(60);

    loop {
        // If the PM WS reconnected, force an immediate scanner poll + REST
        // re-fetch so LatestPrices doesn't stay stale for the scan interval.
        if pm_needs_refresh.swap(false, std::sync::atomic::Ordering::Relaxed) {
            info!("PM WS reconnected — forcing immediate REST orderbook re-fetch");
            next_scan_at = tokio::time::Instant::now();
        }

        tokio::select! {
            () = shutdown.cancelled() => {
                info!("shutdown signal received — exiting main loop");
                break;
            }

            // ── Receive events from the PM WebSocket task ────────────────
            Some(pm_event) = pm_event_rx.recv() => {
                match pm_event {
                    PmEvent::BestBidAsk { token_id, best_bid, best_ask, timestamp_ms } => {
                        // Update local OrderbookTracker (condition_id-based).
                        local_tracker.update(&token_id, "SELL", best_ask, timestamp_ms);
                        local_tracker.update(&token_id, "BUY", best_bid, timestamp_ms);

                        // Update local LatestPrices cache (Asset, Timeframe-based).
                        // Skip resolved-market prices.
                        if best_ask > 0.01 && best_ask < 0.99 && best_bid > 0.01 && best_bid < 0.99 {
                            let lookup = match token_asset_map.lock() {
                                Ok(map) => map.get(&token_id).copied(),
                                Err(e) => {
                                    warn!(error = %e, "token_asset_map mutex poisoned");
                                    None
                                }
                            };
                            if let Some((asset, timeframe, is_up)) = lookup {
                                local_prices.update_side(asset, timeframe, is_up, best_bid, best_ask, timestamp_ms);
                            }
                        }
                    }
                    PmEvent::Book { token_id, bids, asks, timestamp_ms } => {
                        // Convert BookLevels to a BookEvent for the L2 manager.
                        let book_event = pm_market::l2_orderbook::BookEvent {
                            event_type: "book".to_string(),
                            asset_id: token_id,
                            bids,
                            asks,
                            timestamp: timestamp_ms.to_string(),
                        };
                        local_l2.process_book_event(&book_event.asset_id, &book_event, timestamp_ms);
                    }
                    PmEvent::PriceChange { token_id, changes, timestamp_ms } => {
                        local_l2.process_price_change(&token_id, &changes, timestamp_ms);
                    }
                    PmEvent::MarketResolved { condition_id, winning_token_id, timestamp_ms } => {
                        info!(
                            condition_id = %condition_id,
                            winning_token = %winning_token_id,
                            "PM event: market_resolved"
                        );

                        let matched = market_mgr.active_markets().find(|m| {
                            m.condition_id == condition_id
                        });

                        let Some(mkt) = matched else {
                            warn!(
                                condition_id = %condition_id,
                                "market_resolved for unknown condition — ignoring"
                            );
                            continue;
                        };

                        let outcome = if winning_token_id == mkt.token_id_up {
                            Side::Up
                        } else {
                            Side::Down
                        };

                        let asset_idx = enabled_assets.iter().position(|a| *a == mkt.asset);
                        let tf_idx = enabled_timeframes.iter().position(|t| *t == mkt.timeframe);

                        if let (Some(ai), Some(ti)) = (asset_idx, tf_idx) {
                            let slot = ai * enabled_timeframes.len() + ti;
                            if let Some(lw) = live_windows[slot].as_mut() {
                                let window_id = lw.window.id;
                                let window_pnl = executor.resolve_window(
                                    window_id,
                                    outcome,
                                    timestamp_ms,
                                );
                                risk.on_window_resolved(window_id, window_pnl);
                                lw.position_opened = true;

                                info!(
                                    condition_id = %condition_id,
                                    asset = %mkt.asset,
                                    timeframe = ?mkt.timeframe,
                                    %outcome,
                                    pnl = window_pnl.as_f64(),
                                    "market resolved early — positions closed"
                                );
                            }
                        }
                    }
                }
            }

            () = tokio::time::sleep_until(next_scan_at) => {
                match scan_active_markets(&http_client, &enabled_assets).await {
                    Ok(markets) => {
                        info!(count = markets.len(), "market scan completed");

                        let mut new_token_ids: Vec<String> = Vec::new();
                        for m in &markets {
                            if subscribed_tokens.insert(m.token_id_up.clone()) {
                                new_token_ids.push(m.token_id_up.clone());
                            }
                            if subscribed_tokens.insert(m.token_id_down.clone()) {
                                new_token_ids.push(m.token_id_down.clone());
                            }
                        }

                        for m in &markets {
                            local_tracker.register_market(
                                &m.condition_id,
                                &m.token_id_up,
                                &m.token_id_down,
                            );
                        }

                        // Populate token → (Asset, Timeframe, is_up) map so the
                        // PM WS handler can route events to LatestPrices.
                        match token_asset_map.lock() {
                            Ok(mut map) => {
                                for m in &markets {
                                    map.insert(
                                        m.token_id_up.clone(),
                                        (m.asset, m.timeframe, true),
                                    );
                                    map.insert(
                                        m.token_id_down.clone(),
                                        (m.asset, m.timeframe, false),
                                    );
                                }
                            }
                            Err(e) => {
                                warn!(error = %e, "token_asset_map mutex poisoned — skipping update");
                            }
                        }

                        market_mgr.update_markets(markets.clone());

                        // Fetch REST orderbook snapshots only for NEW tokens so the
                        // tracker has initial state immediately.  Already-subscribed
                        // markets receive live updates via the PM WebSocket, so
                        // re-fetching them would be wasteful (~314 REST calls/scan).
                        if !new_token_ids.is_empty() {
                            #[expect(clippy::cast_possible_truncation, reason = "millis since epoch fits in u64 for centuries")]
                            let now_ms = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64;

                            for token_id in &new_token_ids {
                                fetch_rest_orderbook(
                                    &http_client,
                                    token_id,
                                    &mut local_tracker,
                                    now_ms,
                                ).await;
                            }

                            info!(
                                count = new_token_ids.len(),
                                "REST orderbook snapshots fetched for new tokens"
                            );
                        }

                        if !new_token_ids.is_empty() {
                            info!(
                                count = new_token_ids.len(),
                                "subscribing PM WS to new token IDs"
                            );
                            if let Err(e) = pm_new_tokens_tx.send(new_token_ids).await {
                                warn!(error = %e, "failed to send new token IDs to PM WS task");
                            }
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "market scan failed — retrying next interval");
                    }
                }
                next_scan_at = tokio::time::Instant::now() + market_mgr.scanner_interval;
            }

            tick_result = tick_rx.recv() => {
                let tick = match tick_result {
                    Ok(t) => t,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(dropped = n, "tick channel lagged — some ticks lost");
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("tick channel closed");
                        break;
                    }
                };

                if !FIRST_TICK_LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    info!(
                        asset = %tick.asset,
                        price = tick.price.as_f64(),
                        source = %tick.source,
                        "first tick received from WebSocket"
                    );
                }

                let Some(tick) = oracle_router.process(tick) else {
                    continue;
                };

                let Some(asset_slot) = enabled_assets.iter().position(|a| *a == tick.asset) else {
                    continue;
                };

                price_buffer.push(tick.asset, tick.timestamp_ms, tick.price);

                let n = TICK_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if n.is_multiple_of(100) && n > 0 {
                    info!(
                        ticks_processed = n,
                        asset = %tick.asset,
                        price = tick.price.as_f64(),
                        "tick throughput"
                    );
                }

                // Compute once per tick — passed into process_tick to avoid
                // repeated Utc::now() syscalls inside the timeframe loop.
                let now_utc = chrono::Utc::now();

                process_tick(
                    &tick,
                    &enabled_timeframes,
                    asset_slot,
                    slippage,
                    now_utc,
                    &mut live_windows,
                    &mut executor,
                    &mut risk,
                    &local_tracker,
                    &local_prices,
                    &market_mgr,
                    &mut recorder,
                    &mut window_recorder,
                    &engine,
                    &mut ema_tracker,
                    &trend_filter,
                    &local_l2,
                    entry_timer_opt.as_ref(),
                    &mut stats,
                );

                // ── Periodic session summary (every 5 min) ─────────────────
                if stats.last_summary_at.elapsed() >= std::time::Duration::from_secs(300) {
                    let open_pos = risk.open_position_count();
                    stats.log_summary(executor.balance(), open_pos);
                }

                // ── Periodic cleanup of expired positions ───────────────────
                if tokio::time::Instant::now() >= next_cleanup_at {
                    let cleanup_pnl = executor.cleanup_expired_positions(
                        tick.timestamp_ms,
                        MAX_WINDOW_DURATION_MS,
                    );
                    if cleanup_pnl.as_f64().abs() > f64::EPSILON {
                        warn!(
                            pnl = cleanup_pnl.as_f64(),
                            "expired positions cleaned up"
                        );
                    }
                    next_cleanup_at = tokio::time::Instant::now() + Duration::from_secs(60);
                }

                // Market resolution events are now handled via the pm_event_rx
                // channel branch above — no mutex drain needed.
            }
        }
    }

    // ── 7. Drain any remaining PM events ──────────────────────────────────────
    while let Ok(event) = pm_event_rx.try_recv() {
        if let PmEvent::MarketResolved { condition_id, winning_token_id, .. } = event {
            info!(
                condition_id = %condition_id,
                winning_token = %winning_token_id,
                "draining final market_resolved event on shutdown"
            );
        }
    }

    // ── 8. Flush recorder and print summary ───────────────────────────────────
    if let Err(e) = recorder.flush() {
        warn!(error = %e, "failed to flush recorder on shutdown");
    }

    info!(
        open_positions = executor.open_position_count(),
        "paper trading session ended"
    );
    executor.print_summary();

    Ok(())
}
