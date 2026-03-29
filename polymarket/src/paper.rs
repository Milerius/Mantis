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

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context as _, Result};
use pm_bookkeeper::SnapshotRecorder;
use pm_executor::{PaperConfig, PaperExecutor};
use pm_market::{MarketManager, OrderbookTracker, PolymarketWs};
use pm_market::scanner::scan_active_markets;
use pm_oracle::{BinanceWs, OkxWs, OracleRouter, PriceBuffer};
use pm_risk::{RiskConfig, RiskManager};
use pm_signal::{AnyStrategy, CompleteSetArb, EarlyDirectional, HedgeLock, MomentumConfirmation, StrategyEngine};
use pm_types::{
    Asset, ContractPrice, MarketState, OpenPosition, Pnl, Side, Timeframe, Tick, Window, WindowId,
    config::BotConfig,
};
use reqwest::Client;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

// ─── Window tracking ─────────────────────────────────────────────────────────

/// Per-(asset, timeframe) window state updated on each tick.
struct LiveWindow {
    window: Window,
    /// Whether a position has already been opened in this window.
    position_opened: bool,
}

// ─── run_paper ────────────────────────────────────────────────────────────────

/// Run the paper trading loop.
///
/// Connects to Binance and OKX WebSockets, polls the Gamma API for active
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
    };
    let mut risk = RiskManager::new(risk_config);

    let engine = StrategyEngine::from_any(vec![
        AnyStrategy::Early(EarlyDirectional::new(300, 0.005, 0.65)),
        AnyStrategy::Momentum(MomentumConfirmation::new(300, 900, 0.005, 0.65)),
        AnyStrategy::Arb(CompleteSetArb::new(0.98, 0.015)),
        AnyStrategy::Hedge(HedgeLock::new(0.98)),
    ]);

    let mut oracle_router = OracleRouter::new();
    let mut price_buffer = PriceBuffer::new();

    // Per-(asset, timeframe) window table.
    let num_slots = enabled_assets.len() * enabled_timeframes.len();
    let mut live_windows: Vec<Option<LiveWindow>> = (0..num_slots).map(|_| None).collect();

    // ── Shared orderbook tracker for PM WebSocket ─────────────────────────────
    let shared_tracker: Arc<Mutex<OrderbookTracker>> =
        Arc::new(Mutex::new(OrderbookTracker::new()));

    let mut market_mgr = MarketManager::new(Duration::from_secs(30));
    let http_client = Client::new();
    let mut next_scan_at = tokio::time::Instant::now();

    let mut subscribed_tokens: HashSet<String> = HashSet::new();

    let (pm_ws, pm_new_tokens_tx) = PolymarketWs::new(Vec::new());
    let pm_tracker = Arc::clone(&shared_tracker);
    let pm_shutdown = shutdown.clone();
    tokio::spawn(async move {
        pm_ws.run(pm_tracker, pm_shutdown).await;
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

    // ── 5. Graceful shutdown signal ───────────────────────────────────────────
    let ctrlc_shutdown = shutdown.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for ctrl-c");
        info!("SIGINT received — shutting down");
        ctrlc_shutdown.cancel();
    });

    // ── 6. Main event loop ────────────────────────────────────────────────────
    let slippage = f64::from(cfg.backtest.slippage_bps) * 0.0001;

    loop {
        tokio::select! {
            () = shutdown.cancelled() => {
                info!("shutdown signal received — exiting main loop");
                break;
            }

            _ = tokio::time::sleep_until(next_scan_at) => {
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

                        if let Ok(mut tracker) = shared_tracker.lock() {
                            for m in &markets {
                                tracker.register_market(
                                    &m.condition_id,
                                    &m.token_id_up,
                                    &m.token_id_down,
                                );
                            }
                        }

                        market_mgr.update_markets(markets.clone());

                        // Fetch REST orderbook snapshots for all discovered markets so the
                        // tracker has initial state immediately — even on quiet markets where
                        // the PM WebSocket won't fire until the next book change.
                        let now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64;

                        for m in &markets {
                            // ── Up token ─────────────────────────────────────────────────
                            let up_url = format!(
                                "https://clob.polymarket.com/book?token_id={}",
                                m.token_id_up
                            );
                            match http_client.get(&up_url).send().await {
                                Ok(resp) => match resp.json::<serde_json::Value>().await {
                                    Ok(book) => {
                                        if let Some(asks) =
                                            book.get("asks").and_then(|a| a.as_array())
                                        {
                                            if let Some(best) = asks.first() {
                                                let price: f64 = best
                                                    .get("price")
                                                    .and_then(|p| p.as_str())
                                                    .and_then(|s| s.parse().ok())
                                                    .unwrap_or(0.0);
                                                if price > 0.01 && price < 0.99 {
                                                    if let Ok(mut tracker) = shared_tracker.lock() {
                                                        tracker.update(
                                                            &m.token_id_up,
                                                            "SELL",
                                                            price,
                                                            now_ms,
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                        if let Some(bids) =
                                            book.get("bids").and_then(|a| a.as_array())
                                        {
                                            if let Some(best) = bids.first() {
                                                let price: f64 = best
                                                    .get("price")
                                                    .and_then(|p| p.as_str())
                                                    .and_then(|s| s.parse().ok())
                                                    .unwrap_or(0.0);
                                                if price > 0.01 && price < 0.99 {
                                                    if let Ok(mut tracker) = shared_tracker.lock() {
                                                        tracker.update(
                                                            &m.token_id_up,
                                                            "BUY",
                                                            price,
                                                            now_ms,
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        warn!(
                                            token_id = %m.token_id_up,
                                            error = %e,
                                            "failed to parse REST orderbook for Up token"
                                        );
                                    }
                                },
                                Err(e) => {
                                    warn!(
                                        token_id = %m.token_id_up,
                                        error = %e,
                                        "REST orderbook fetch failed for Up token"
                                    );
                                }
                            }

                            // ── Down token ───────────────────────────────────────────────
                            let down_url = format!(
                                "https://clob.polymarket.com/book?token_id={}",
                                m.token_id_down
                            );
                            match http_client.get(&down_url).send().await {
                                Ok(resp) => match resp.json::<serde_json::Value>().await {
                                    Ok(book) => {
                                        if let Some(asks) =
                                            book.get("asks").and_then(|a| a.as_array())
                                        {
                                            if let Some(best) = asks.first() {
                                                let price: f64 = best
                                                    .get("price")
                                                    .and_then(|p| p.as_str())
                                                    .and_then(|s| s.parse().ok())
                                                    .unwrap_or(0.0);
                                                if price > 0.01 && price < 0.99 {
                                                    if let Ok(mut tracker) = shared_tracker.lock() {
                                                        tracker.update(
                                                            &m.token_id_down,
                                                            "SELL",
                                                            price,
                                                            now_ms,
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                        if let Some(bids) =
                                            book.get("bids").and_then(|a| a.as_array())
                                        {
                                            if let Some(best) = bids.first() {
                                                let price: f64 = best
                                                    .get("price")
                                                    .and_then(|p| p.as_str())
                                                    .and_then(|s| s.parse().ok())
                                                    .unwrap_or(0.0);
                                                if price > 0.01 && price < 0.99 {
                                                    if let Ok(mut tracker) = shared_tracker.lock() {
                                                        tracker.update(
                                                            &m.token_id_down,
                                                            "BUY",
                                                            price,
                                                            now_ms,
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        warn!(
                                            token_id = %m.token_id_down,
                                            error = %e,
                                            "failed to parse REST orderbook for Down token"
                                        );
                                    }
                                },
                                Err(e) => {
                                    warn!(
                                        token_id = %m.token_id_down,
                                        error = %e,
                                        "REST orderbook fetch failed for Down token"
                                    );
                                }
                            }

                            info!(
                                condition_id = %m.condition_id,
                                token_up = %m.token_id_up,
                                token_down = %m.token_id_down,
                                "REST orderbook snapshot fetched"
                            );
                        }

                        if !new_token_ids.is_empty() {
                            info!(
                                count = new_token_ids.len(),
                                "subscribing PM WS to new token IDs"
                            );
                            if let Err(e) = pm_new_tokens_tx.try_send(new_token_ids) {
                                warn!(error = %e, "failed to send new token IDs to PM WS task");
                            }
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "market scan failed — retrying in 30s");
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

                static FIRST_TICK_LOGGED: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
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

                static TICK_COUNT: std::sync::atomic::AtomicU64 =
                    std::sync::atomic::AtomicU64::new(0);
                let n = TICK_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if n.is_multiple_of(100) && n > 0 {
                    info!(
                        ticks_processed = n,
                        asset = %tick.asset,
                        price = tick.price.as_f64(),
                        "tick throughput"
                    );
                }

                for (tf_idx, &timeframe) in enabled_timeframes.iter().enumerate() {
                    let slot = asset_slot * enabled_timeframes.len() + tf_idx;
                    let duration_ms = timeframe.duration_secs() * 1_000;
                    let window_open_ms = tick.timestamp_ms - (tick.timestamp_ms % duration_ms);
                    let window_close_ms = window_open_ms + duration_ms;

                    let need_new = live_windows[slot]
                        .as_ref()
                        .is_none_or(|lw| tick.timestamp_ms >= lw.window.close_time_ms);

                    if need_new {
                        if let Some(old_lw) = live_windows[slot].take() {
                            let outcome = old_lw.window.direction(tick.price);

                            // Notify risk manager about the resolved window.
                            // Use zero PnL here — executor tracks actual P&L;
                            // risk manager only needs the event to clear positions.
                            risk.on_window_resolved(old_lw.window.id, Pnl::ZERO);

                            executor.resolve_window(old_lw.window.id, outcome, tick.timestamp_ms);

                            info!(
                                asset = %tick.asset,
                                timeframe = ?timeframe,
                                window_id = %old_lw.window.id,
                                outcome = %outcome,
                                "window resolved"
                            );
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
                        });
                        info!(
                            asset = %tick.asset,
                            timeframe = ?timeframe,
                            window_id = %new_window.id,
                            open_price = tick.price.as_f64(),
                            "new window opened"
                        );
                    }

                    let Some(lw) = live_windows[slot].as_mut() else {
                        continue;
                    };

                    if lw.position_opened {
                        continue;
                    }

                    let window = lw.window;
                    let magnitude = window.magnitude(tick.price);
                    let time_elapsed_secs =
                        (tick.timestamp_ms.saturating_sub(window.open_time_ms)) / 1_000;
                    let time_remaining_secs = window.time_remaining_secs(tick.timestamp_ms);
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

                    // Match only a market whose window has NOT yet ended so we
                    // never bind to a recently-resolved market that still appears
                    // in the scanner list with stale orderbook prices.
                    let now_utc = chrono::Utc::now();
                    let condition_id_opt = market_mgr
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
                        .map(|m| m.condition_id.clone());

                    let ob_snap = condition_id_opt.as_deref().and_then(|cid| {
                        if let Ok(tracker) = shared_tracker.lock() {
                            if let Some(snap) = tracker.get(cid) {
                                if snap.ask_up.is_some() || snap.ask_down.is_some() {
                                    return Some(*snap);
                                }
                            }
                        }
                        market_mgr.orderbook(cid).copied()
                    });

                    // Resolve orderbook prices — prefer live PM WS, fall back to model.
                    // rec_* are Option<f64> for the recorder; contract_* are Option<ContractPrice>
                    // for MarketState (which already uses Option internally).
                    let (rec_ask_up, rec_ask_down, rec_bid_up, rec_bid_down,
                         contract_ask_up, contract_ask_down, contract_bid_up, contract_bid_down) =
                        match ob_snap {
                            Some(snap) if snap.ask_up.is_some() && snap.ask_down.is_some() => {
                                let a_up = snap.ask_up.map_or(0.55, |p| p.as_f64());
                                let a_down = snap.ask_down.map_or(0.48, |p| p.as_f64());
                                let b_up = snap.bid_up.map_or(a_up - 0.02, |p| p.as_f64());
                                let b_down = snap.bid_down.map_or(a_down - 0.02, |p| p.as_f64());

                                // Sanity-check: prices from a resolved market sit at
                                // ~$0.00 or ~$1.00 (fully settled).  Reject anything
                                // outside (0.01, 0.99) for both legs — those are useless
                                // for live trading and would badly mis-price the model.
                                let prices_are_sane = a_up > 0.01 && a_up < 0.99
                                    && a_down > 0.01 && a_down < 0.99;

                                if prices_are_sane {
                                    debug!(
                                        asset = %tick.asset,
                                        timeframe = ?timeframe,
                                        ask_up = a_up,
                                        ask_down = a_down,
                                        "using live PM WS orderbook prices"
                                    );
                                    (
                                        Some(a_up), Some(a_down), Some(b_up), Some(b_down),
                                        ContractPrice::new(a_up),
                                        ContractPrice::new(a_down),
                                        ContractPrice::new(b_up),
                                        ContractPrice::new(b_down),
                                    )
                                } else {
                                    warn!(
                                        asset = %tick.asset,
                                        timeframe = ?timeframe,
                                        ask_up = a_up,
                                        ask_down = a_down,
                                        "PM WS prices look like a resolved market — falling back to model defaults"
                                    );
                                    let base = if spot_direction == Side::Up { 0.55 } else { 0.48 };
                                    let opp = 1.0 - base + slippage;
                                    (
                                        None, None, None, None,
                                        ContractPrice::new(base.clamp(0.01, 0.99)),
                                        ContractPrice::new(opp.clamp(0.01, 0.99)),
                                        ContractPrice::new((base - 0.02).clamp(0.01, 0.99)),
                                        ContractPrice::new((opp - 0.02).clamp(0.01, 0.99)),
                                    )
                                }
                            }
                            _ => {
                                let base = if spot_direction == Side::Up { 0.55 } else { 0.48 };
                                let opp = 1.0 - base + slippage;
                                (
                                    None, None, None, None,
                                    ContractPrice::new(base.clamp(0.01, 0.99)),
                                    ContractPrice::new(opp.clamp(0.01, 0.99)),
                                    ContractPrice::new((base - 0.02).clamp(0.01, 0.99)),
                                    ContractPrice::new((opp - 0.02).clamp(0.01, 0.99)),
                                )
                            }
                        };

                    let state = MarketState {
                        asset: tick.asset,
                        timeframe,
                        window_id: window.id,
                        window_open_price: window.open_price,
                        current_spot: tick.price,
                        spot_magnitude: magnitude,
                        spot_direction,
                        time_elapsed_secs,
                        time_remaining_secs,
                        contract_ask_up,
                        contract_ask_down,
                        contract_bid_up,
                        contract_bid_down,
                    };

                    // Record combined snapshot after building MarketState — all data available here.
                    if let Err(e) = recorder.record(
                        tick.timestamp_ms,
                        &tick.asset.to_string(),
                        tick.price.as_f64(),
                        rec_ask_up,
                        rec_ask_down,
                        rec_bid_up,
                        rec_bid_down,
                        window_open_ms,
                        timeframe.duration_secs(),
                    ) {
                        warn!(error = %e, "failed to record snapshot");
                    }

                    let decisions = engine.evaluate_all(&state);

                    for decision in &decisions {
                        info!(
                            asset = %tick.asset,
                            timeframe = ?timeframe,
                            side = %decision.side,
                            strategy = %decision.strategy_id,
                            confidence = decision.confidence,
                            limit_price = decision.limit_price.as_f64(),
                            "strategy signal fired"
                        );

                        // Run decision through risk manager before opening.
                        let sized_order = match risk.evaluate(
                            decision,
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
                                    "risk manager rejected entry"
                                );
                                continue;
                            }
                        };

                        if let Some(fill) = executor.try_open_position(
                            decision,
                            window.id,
                            tick.asset,
                            tick.timestamp_ms,
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
                            info!(
                                asset = %tick.asset,
                                side = %decision.side,
                                fill_price = fill.fill_price.as_f64(),
                                size_usdc = fill.size_usdc,
                                balance = executor.balance(),
                                "paper fill executed"
                            );
                            // Only one position per window.
                            break;
                        }
                    }
                }
            }
        }
    }

    // ── 7. Flush recorder and print summary ───────────────────────────────────
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
