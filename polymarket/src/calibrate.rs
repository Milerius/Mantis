//! Calibrate subcommand: build an empirical [`LookupTable`] from historical data.
//!
//! Algorithm:
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

use std::path::Path;

use anyhow::{Context as _, Result};
use pm_oracle::HistoricalReplay;
use pm_oracle::downloader::date_range;
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

/// Process all ticks from `replay`, accumulating per-cell Up/total counts.
fn accumulate_cells(
    replay: HistoricalReplay,
    enabled_assets: &[Asset],
    enabled_timeframes: &[Timeframe],
    cells: &mut [Cell],
) {
    let slot_count = Asset::COUNT * Timeframe::COUNT;
    let mut windows: Vec<Option<Window>> = vec![None; slot_count];
    let mut pending: Vec<Vec<Observation>> = vec![Vec::new(); slot_count];
    let mut window_id_counter: u64 = 1;

    for tick in replay {
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
    // Observations for windows that never closed are discarded — outcome unknown.
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

// ─── Public API ──────────────────────────────────────────────────────────────

/// Build a calibrated [`LookupTable`] from the training portion of cached data.
///
/// `dates` should contain only the **training** dates (the first 60 % of the
/// overall date range).
///
/// # Errors
///
/// Returns an error if any cache file is missing or cannot be read.
pub fn run_calibrate(cfg: &BotConfig, dates: &[String]) -> Result<LookupTable> {
    let cache_dir = Path::new(&cfg.data.cache_dir);
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

    let total_cells = Asset::COUNT * Timeframe::COUNT * MAG_BUCKETS * TIME_BUCKETS;
    let mut cells: Vec<Cell> = vec![Cell::default(); total_cells];
    accumulate_cells(replay, &enabled_assets, &enabled_timeframes, &mut cells);

    let table = build_table(&cells);
    info!("calibration complete");
    Ok(table)
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
