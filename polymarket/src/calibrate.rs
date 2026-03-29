//! Calibrate subcommand: build empirical models from historical data.
//!
//! Produces two models:
//!
//! 1. **[`LookupTable`]** — empirical P(Up) for each `(asset, timeframe,
//!    magnitude_bucket, time_remaining_bucket)` cell.  Used by the legacy
//!    signal engine to estimate outcome probability.
//!
//! 2. **[`ContractPriceModel`]** — empirical median Polymarket contract price
//!    for each `(asset, timeframe, magnitude_bucket, time_elapsed_bucket)` cell.
//!    Built from paired Binance spot ticks + Polymarket 15m trade data.
//!
//! ## Algorithm (`LookupTable`)
//!
//! 1. Load cached candles as a time-sorted tick stream via [`HistoricalReplay`].
//! 2. Maintain one open [`Window`] per `(asset, timeframe)` slot.
//! 3. For every tick inside an open window, record an observation:
//!    `(magnitude, time_remaining_secs, asset, timeframe)`.
//! 4. When a new window boundary is crossed, resolve the old window (Up or Down)
//!    and tag every observation that belonged to it with the outcome.
//! 5. After all ticks are processed, group observations into
//!    `(asset, timeframe, mag_bucket, time_bucket)` cells and compute the
//!    empirical P(Up) for each cell.
//! 6. Write the results into a [`LookupTable`] and return it.
//!
//! ## Algorithm (`ContractPriceModel`)
//!
//! For each 15m window where both Binance spot ticks and Polymarket trade
//! records are present:
//! 1. At each spot tick timestamp, compute `spot_magnitude`.
//! 2. Find the closest-in-time Polymarket trade for the Up or Down outcome.
//! 3. Record the pair as a [`PriceObservation`].
//! 4. Feed all observations into [`contract_model::calibrate`] to produce the
//!    [`ContractPriceModel`].

use std::path::Path;

use anyhow::{Context as _, Result};
use pm_oracle::HistoricalReplay;
use pm_oracle::contract_model::{self, ContractPriceModel, PriceObservation};
use pm_oracle::downloader::date_range;
use pm_oracle::polymarket::{market_slugs, read_polymarket_trades};
use pm_signal::{LookupTable, MAG_BUCKETS, TIME_BUCKETS};
use pm_types::{Asset, ExchangeSource, Side, Timeframe, Window, WindowId, config::BotConfig};
use tracing::info;

// ─── Internal types ───────────────────────────────────────────────────────────

/// A single data point recorded while a window was open.
#[derive(Clone, Copy)]
struct Observation {
    asset: Asset,
    timeframe: Timeframe,
    mag_bucket: usize,
    time_bucket: usize,
}

/// Per-cell counts for computing P(Up).
#[derive(Clone, Copy, Default)]
struct Cell {
    up: u32,
    total: u32,
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Flat cell index for `(asset, timeframe, mag_bucket, time_bucket)`.
#[inline]
fn cell_index(asset: Asset, tf: Timeframe, mag_bucket: usize, time_bucket: usize) -> usize {
    asset.index() * (Timeframe::COUNT * MAG_BUCKETS * TIME_BUCKETS)
        + tf.index() * (MAG_BUCKETS * TIME_BUCKETS)
        + mag_bucket * TIME_BUCKETS
        + time_bucket
}

/// Collect all unique enabled timeframes across all assets in `cfg`.
fn collect_timeframes(cfg: &BotConfig) -> Vec<Timeframe> {
    let mut tfs: Vec<Timeframe> = Vec::new();
    for ac in &cfg.bot.assets {
        if !ac.enabled {
            continue;
        }
        for &tf in &ac.timeframes {
            if !tfs.contains(&tf) {
                tfs.push(tf);
            }
        }
    }
    tfs
}

/// Commit observations from a resolved window into the cell accumulator.
fn commit_observations(observations: &mut Vec<Observation>, cells: &mut [Cell], outcome: Side) {
    let is_up = outcome == Side::Up;
    for obs in observations.drain(..) {
        let idx = cell_index(obs.asset, obs.timeframe, obs.mag_bucket, obs.time_bucket);
        cells[idx].total += 1;
        if is_up {
            cells[idx].up += 1;
        }
    }
}

/// Build a [`LookupTable`] from the accumulated cell counts.
fn build_table(cells: &[Cell]) -> LookupTable {
    let min_samples: u32 = 10;
    let mut table = LookupTable::new(min_samples);
    for asset in Asset::ALL {
        for tf in Timeframe::ALL {
            for mb in 0..MAG_BUCKETS {
                for tb in 0..TIME_BUCKETS {
                    let idx = cell_index(asset, tf, mb, tb);
                    let cell = cells[idx];
                    if cell.total > 0 {
                        let prob = f64::from(cell.up) / f64::from(cell.total);
                        table.set(asset, tf, mb, tb, prob, cell.total);
                    }
                }
            }
        }
    }
    table
}

// ─── ContractPriceModel calibration ──────────────────────────────────────────

/// Build a [`ContractPriceModel`] from paired spot ticks and Polymarket trade data.
///
/// For each 15m window in `dates`, we:
/// 1. Load every Binance spot tick that falls within that window.
/// 2. Load all Polymarket trades for the same window slug.
/// 3. For each spot tick, find the closest Polymarket trade by timestamp.
/// 4. Record the pair as a [`PriceObservation`].
///
/// Only the 15m timeframe is used — it has the most reliable slug coverage.
///
/// Returns an empty model (all cells have `sample_count = 0`) if no Polymarket
/// data is cached.
fn build_contract_model(
    replay_ticks: &[pm_types::Tick],
    enabled_assets: &[Asset],
    pm_cache_dir: &Path,
    dates: &[String],
    min_samples: u32,
) -> ContractPriceModel {
    let tf = Timeframe::Min15;
    let window_secs = tf.duration_secs();
    let window_ms = window_secs * 1_000;

    let mut observations: Vec<PriceObservation> = Vec::new();

    for &asset in enabled_assets {
        // Generate all (slug, epoch_secs) pairs for this asset over the date range.
        let slugs = match market_slugs(asset, tf, dates) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(asset = %asset, error = %e, "failed to generate slugs");
                continue;
            }
        };

        for (slug, epoch_secs) in &slugs {
            let window_open_ms = epoch_secs * 1_000;
            let window_close_ms = window_open_ms + window_ms;

            // Read cached Polymarket trades for this window.
            // Not cached yet — skip silently.
            let Ok(pm_trades) = read_polymarket_trades(pm_cache_dir, slug) else {
                continue;
            };

            if pm_trades.is_empty() {
                continue;
            }

            // Collect spot ticks that fall within this window.
            let window_ticks: Vec<&pm_types::Tick> = replay_ticks
                .iter()
                .filter(|t| {
                    t.asset == asset
                        && t.timestamp_ms >= window_open_ms
                        && t.timestamp_ms < window_close_ms
                })
                .collect();

            if window_ticks.is_empty() {
                continue;
            }

            // For each spot tick, find the closest trade and record an observation.
            let open_price = window_ticks[0].price;

            for tick in &window_ticks {
                // Find the closest Polymarket trade by timestamp (in seconds).
                let tick_ts_secs = tick.timestamp_ms / 1_000;
                let closest = pm_trades.iter().min_by_key(|t| {
                    #[expect(
                        clippy::cast_possible_wrap,
                        reason = "timestamps are in seconds since epoch; both fit well within i64"
                    )]
                    (t.timestamp as i64 - tick_ts_secs as i64).unsigned_abs()
                });

                let Some(trade) = closest else { continue };

                // Only use Up-outcome trades so we record the Up contract price
                // (the natural complement for directional strategies).
                if trade.outcome != "Up" {
                    continue;
                }

                // Compute spot magnitude relative to window open.
                let open_f = open_price.as_f64();
                if open_f <= 0.0 {
                    continue;
                }
                let magnitude = ((tick.price.as_f64() - open_f) / open_f).abs();
                let time_elapsed_secs = tick.timestamp_ms.saturating_sub(window_open_ms) / 1_000;

                // Only include if price is a plausible contract price [0, 1].
                if !(0.0..=1.0).contains(&trade.price) {
                    continue;
                }

                observations.push(PriceObservation {
                    asset,
                    timeframe: tf,
                    magnitude,
                    time_elapsed_secs,
                    contract_price: trade.price,
                });
            }
        }
    }

    info!(
        observations = observations.len(),
        "contract model observations collected"
    );
    contract_model::calibrate(&observations, min_samples)
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Calibration output: both the legacy [`LookupTable`] and the new
/// [`ContractPriceModel`].
///
/// The `contract_model` field is built for future use by a `ContractPriceModelProvider`
/// that can replace the fixed price provider in the sweep and backtest commands.
pub struct CalibrationResult {
    /// Empirical P(Up) lookup table (legacy signal engine).
    pub table: LookupTable,
    /// Empirical contract price model (paired spot + Polymarket data).
    pub contract_model: ContractPriceModel,
}

/// Build a calibrated [`LookupTable`] and [`ContractPriceModel`] from the
/// training portion of cached data.
///
/// `dates` should contain only the **training** dates (the first 60 % of the
/// overall date range).
///
/// # Errors
///
/// Returns an error if any Binance cache file is missing or cannot be read.
/// Missing Polymarket cache files are silently skipped — the contract model
/// will simply have fewer observations.
pub fn run_calibrate(cfg: &BotConfig, dates: &[String]) -> Result<CalibrationResult> {
    let cache_dir = Path::new(&cfg.data.cache_dir);
    let pm_cache_dir = cache_dir.join("polymarket");
    let enabled_assets: Vec<Asset> = cfg
        .bot
        .assets
        .iter()
        .filter(|a| a.enabled)
        .map(|a| a.asset)
        .collect();
    let enabled_timeframes = collect_timeframes(cfg);

    info!(
        assets     = ?enabled_assets,
        timeframes = ?enabled_timeframes,
        days       = dates.len(),
        "calibrating from historical data"
    );

    let replay = HistoricalReplay::load(cache_dir, &enabled_assets, ExchangeSource::Binance, dates)
        .context("failed to load historical data for calibration")?;

    // Collect all ticks for the contract model before consuming `replay`.
    let all_ticks: Vec<pm_types::Tick> = replay.collect();

    let total_cells = Asset::COUNT * Timeframe::COUNT * MAG_BUCKETS * TIME_BUCKETS;
    let mut cells: Vec<Cell> = vec![Cell::default(); total_cells];

    // Re-iterate the collected ticks for the LookupTable accumulation pass.
    // Tick is Copy so `.iter().copied()` is cheap.
    accumulate_cells_from_iter(
        all_ticks.iter().copied(),
        &enabled_assets,
        &enabled_timeframes,
        &mut cells,
    );

    let table = build_table(&cells);

    // Build ContractPriceModel from paired spot + Polymarket data.
    let contract_model = build_contract_model(
        &all_ticks,
        &enabled_assets,
        &pm_cache_dir,
        dates,
        /* min_samples */ 5,
    );

    info!(
        lookup_table_ready = true,
        contract_model_ready = true,
        "calibration complete"
    );

    Ok(CalibrationResult {
        table,
        contract_model,
    })
}

/// Variant of `accumulate_cells` that takes an arbitrary tick iterator.
fn accumulate_cells_from_iter(
    ticks: impl Iterator<Item = pm_types::Tick>,
    enabled_assets: &[Asset],
    enabled_timeframes: &[Timeframe],
    cells: &mut [Cell],
) {
    let slot_count = Asset::COUNT * Timeframe::COUNT;
    let mut windows: Vec<Option<Window>> = vec![None; slot_count];
    let mut pending: Vec<Vec<Observation>> = vec![Vec::new(); slot_count];
    let mut window_id_counter: u64 = 1;

    for tick in ticks {
        if !enabled_assets.contains(&tick.asset) {
            continue;
        }
        let asset_idx = tick.asset.index();

        for &tf in enabled_timeframes {
            let slot = asset_idx * Timeframe::COUNT + tf.index();
            let duration_ms = tf.duration_secs() * 1_000;
            let window_open_ms = tick.timestamp_ms - (tick.timestamp_ms % duration_ms);
            let window_close_ms = window_open_ms + duration_ms;

            let need_new = windows[slot].is_none_or(|w| tick.timestamp_ms >= w.close_time_ms);
            if need_new {
                if let Some(old) = windows[slot].take() {
                    commit_observations(&mut pending[slot], cells, old.direction(tick.price));
                }
                windows[slot] = Some(Window {
                    id: WindowId::new(window_id_counter),
                    asset: tick.asset,
                    timeframe: tf,
                    open_time_ms: window_open_ms,
                    close_time_ms: window_close_ms,
                    open_price: tick.price,
                });
                window_id_counter += 1;
            }

            if let Some(window) = windows[slot] {
                let mb = LookupTable::mag_bucket(window.magnitude(tick.price));
                let tb = LookupTable::time_bucket(window.time_remaining_secs(tick.timestamp_ms));
                pending[slot].push(Observation {
                    asset: tick.asset,
                    timeframe: tf,
                    mag_bucket: mb,
                    time_bucket: tb,
                });
            }
        }
    }
}

/// Split the configured date range into training (60 %) and test (40 %) slices.
///
/// # Errors
///
/// Returns an error if the date range in `cfg` is invalid.
pub fn split_dates(cfg: &BotConfig) -> Result<(Vec<String>, Vec<String>)> {
    let all = date_range(&cfg.backtest.start_date, &cfg.backtest.end_date)
        .context("invalid date range in config")?;
    let train_len = (all.len() * 60 / 100).max(1);
    let train = all[..train_len].to_vec();
    let test = all[train_len..].to_vec();
    Ok((train, test))
}
