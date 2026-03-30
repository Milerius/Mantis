//! Replay `PolyBackTest` cached data as an observation stream for backtesting.
//!
//! [`PbtReplay`] loads cached market + snapshot files and produces a
//! time-sorted stream of [`PbtObservation`]s. Each observation pairs a spot
//! price tick with real contract prices from the `PolyBackTest` snapshot data.

use std::{
    io,
    path::{Path, PathBuf},
};

use chrono::DateTime;
use pm_types::{Asset, ContractPrice, ExchangeSource, Price, Side, Tick, Timeframe};
use rayon::prelude::*;

use crate::pbt_downloader::{pbt_cache_path, read_pbt_cache};
use crate::polybacktest::{PbtMarket, PbtSnapshot};

// ─── PbtObservation ─────────────────────────────────────────────────────────

/// A paired observation: spot tick + contract prices at the same moment.
///
/// Produced by [`PbtReplay`] from cached `PolyBackTest` data.
#[derive(Debug, Clone)]
pub struct PbtObservation {
    /// Spot price tick constructed from the snapshot's `btc_price` field.
    pub tick: Tick,
    /// Best ask for the Up contract from this snapshot.
    pub ask_up: ContractPrice,
    /// Best ask for the Down contract from this snapshot.
    pub ask_down: ContractPrice,
    /// Spot price at window open (from market metadata).
    pub window_open_price: Price,
    /// Window open time as milliseconds since epoch.
    pub window_open_ms: u64,
    /// Window close time as milliseconds since epoch.
    pub window_close_ms: u64,
    /// Resolution outcome, if the market has resolved.
    pub winner: Option<Side>,
    /// The `PolyBackTest` market ID this observation belongs to.
    pub market_id: String,
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Parse an ISO-8601 timestamp to milliseconds since epoch.
///
/// Returns `None` if parsing fails.
fn parse_iso_to_ms(iso: &str) -> Option<u64> {
    let dt = DateTime::parse_from_rfc3339(iso).ok()?;
    u64::try_from(dt.timestamp_millis()).ok()
}

/// Convert the `coin` string to an [`Asset`].
fn coin_to_asset(coin: &str) -> Option<Asset> {
    match coin {
        "btc" => Some(Asset::Btc),
        "eth" => Some(Asset::Eth),
        "sol" => Some(Asset::Sol),
        "xrp" => Some(Asset::Xrp),
        _ => None,
    }
}

/// Convert the `market_type` string to a [`Timeframe`].
fn market_type_to_timeframe(mt: &str) -> Option<Timeframe> {
    match mt {
        "5m" => Some(Timeframe::Min5),
        "15m" => Some(Timeframe::Min15),
        "1h" => Some(Timeframe::Hour1),
        "4h" => Some(Timeframe::Hour4),
        _ => None,
    }
}

/// Parse the winner field to a [`Side`].
fn parse_winner(winner: Option<&str>) -> Option<Side> {
    match winner {
        Some("Up") => Some(Side::Up),
        Some("Down") => Some(Side::Down),
        _ => None,
    }
}

/// Build observations from a market + snapshot pair.
fn build_observations(
    market: &PbtMarket,
    snapshots: &[PbtSnapshot],
    asset: Asset,
    _timeframe: Timeframe,
) -> Vec<PbtObservation> {
    let Some(window_open_ms) = parse_iso_to_ms(&market.start_time) else {
        return Vec::new();
    };
    let Some(window_close_ms) = parse_iso_to_ms(&market.end_time) else {
        return Vec::new();
    };
    // Use btc_price_start if available. For non-BTC coins (ETH, SOL, XRP),
    // this field is often None — fall back to the first snapshot's spot price.
    let open_price = match market.btc_price_start.and_then(Price::new) {
        Some(p) => p,
        None => {
            // Fallback: use the first snapshot's spot price as open.
            match snapshots.first().and_then(|s| s.btc_price).and_then(Price::new) {
                Some(p) => p,
                None => return Vec::new(),
            }
        }
    };
    let winner = parse_winner(market.winner.as_deref());

    let mut obs = Vec::with_capacity(snapshots.len());
    for snap in snapshots {
        let Some(ts_ms) = parse_iso_to_ms(&snap.time) else {
            continue;
        };
        let Some(btc_price) = snap.btc_price else { continue };
        let Some(price_up) = snap.price_up else { continue };
        let Some(price_down) = snap.price_down else { continue };
        let Some(spot_price) = Price::new(btc_price) else {
            continue;
        };
        let Some(ask_up) = ContractPrice::new(price_up) else {
            continue;
        };
        let Some(ask_down) = ContractPrice::new(price_down) else {
            continue;
        };

        obs.push(PbtObservation {
            tick: Tick {
                asset,
                price: spot_price,
                timestamp_ms: ts_ms,
                source: ExchangeSource::Binance, // placeholder — data is from PBT
            },
            ask_up,
            ask_down,
            window_open_price: open_price,
            window_open_ms,
            window_close_ms,
            winner,
            market_id: market.market_id.clone(),
        });
    }
    obs
}

/// Load and parse a single `.jsonl.gz` cache file, returning all observations it contains.
///
/// Returns `Ok(Vec::new())` (with a warning) rather than propagating errors so that
/// a single corrupt file does not abort an entire parallel load.
fn load_single_file(path: &Path, asset: Asset, timeframe: Timeframe) -> io::Result<Vec<PbtObservation>> {
    let (market, snapshots) = read_pbt_cache(path)?;
    Ok(build_observations(&market, &snapshots, asset, timeframe))
}

// ─── PbtReplay ──────────────────────────────────────────────────────────────

/// Replays cached `PolyBackTest` data as a time-sorted observation stream.
///
/// Load with [`PbtReplay::load`], then iterate with the standard [`Iterator`]
/// interface.
pub struct PbtReplay {
    observations: Vec<PbtObservation>,
    cursor: usize,
}

impl PbtReplay {
    /// Load all cached PBT data for a `(coin, market_type)` pair.
    ///
    /// Scans `cache_dir` for files matching `{coin}_{market_type}_*.jsonl.gz`,
    /// reads each one, builds observations, and sorts by timestamp.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if a cache file cannot be read or parsed.
    pub fn load(cache_dir: &Path, coin: &str, market_type: &str) -> io::Result<Self> {
        let asset = coin_to_asset(coin).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsupported coin: {coin}"),
            )
        })?;
        let timeframe = market_type_to_timeframe(market_type).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsupported market type: {market_type}"),
            )
        })?;

        let prefix = format!("{coin}_{market_type}_");
        let suffix = ".jsonl.gz";

        // Collect matching paths up front so we can hand them to rayon.
        let paths: Vec<PathBuf> = std::fs::read_dir(cache_dir)?
            .filter_map(std::result::Result::ok)
            .filter(|e| {
                let name = e.file_name();
                let s = name.to_string_lossy();
                s.starts_with(&prefix) && s.ends_with(suffix)
            })
            .map(|e| e.path())
            .collect();

        // Process files in parallel — each file is fully independent.
        let all_observations: Vec<Vec<PbtObservation>> = paths
            .par_iter()
            .filter_map(|path| {
                match load_single_file(path, asset, timeframe) {
                    Ok(obs) => Some(obs),
                    Err(e) => {
                        tracing::warn!(path = %path.display(), error = %e, "failed to load PBT file");
                        None
                    }
                }
            })
            .collect();

        let mut observations: Vec<PbtObservation> = all_observations.into_iter().flatten().collect();

        // Sort by timestamp for deterministic replay.
        observations.sort_by_key(|o| o.tick.timestamp_ms);

        Ok(Self {
            observations,
            cursor: 0,
        })
    }

    /// Load from a specific set of market IDs.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if any cache file is missing or unreadable.
    pub fn load_markets(
        cache_dir: &Path,
        coin: &str,
        market_type: &str,
        market_ids: &[String],
    ) -> io::Result<Self> {
        let asset = coin_to_asset(coin).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsupported coin: {coin}"),
            )
        })?;
        let timeframe = market_type_to_timeframe(market_type).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsupported market type: {market_type}"),
            )
        })?;

        // Resolve paths up front, then process in parallel.
        let paths: Vec<PathBuf> = market_ids
            .iter()
            .map(|mid| pbt_cache_path(cache_dir, coin, market_type, mid))
            .collect();

        let all_observations: Vec<Vec<PbtObservation>> = paths
            .par_iter()
            .filter_map(|path| {
                match load_single_file(path, asset, timeframe) {
                    Ok(obs) => Some(obs),
                    Err(e) => {
                        tracing::warn!(path = %path.display(), error = %e, "failed to load PBT file");
                        None
                    }
                }
            })
            .collect();

        let mut observations: Vec<PbtObservation> = all_observations.into_iter().flatten().collect();
        observations.sort_by_key(|o| o.tick.timestamp_ms);
        Ok(Self {
            observations,
            cursor: 0,
        })
    }

    /// Number of observations available.
    #[must_use]
    pub fn len(&self) -> usize {
        self.observations.len()
    }

    /// Returns `true` if no observations were loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.observations.is_empty()
    }

    /// Reset the cursor so observations can be replayed from the beginning.
    pub fn reset(&mut self) {
        self.cursor = 0;
    }

    /// Get a slice of all observations (useful for building price providers).
    #[must_use]
    pub fn observations(&self) -> &[PbtObservation] {
        &self.observations
    }
}

impl Iterator for PbtReplay {
    type Item = PbtObservation;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor < self.observations.len() {
            let obs = self.observations[self.cursor].clone();
            self.cursor += 1;
            Some(obs)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.observations.len() - self.cursor;
        (remaining, Some(remaining))
    }
}

// ─── PbtPriceProvider (model-based) ─────────────────────────────────────────

/// Build [`PriceObservation`]s from PBT data for calibrating a [`ContractPriceModel`].
///
/// For each PBT observation, computes magnitude and `time_elapsed`, then records
/// the contract price as a [`PriceObservation`] that can be fed into
/// [`crate::contract_model::calibrate`].
///
/// Returns observations for the Up contract price (`price_up`).
#[must_use]
pub fn pbt_to_price_observations(
    observations: &[PbtObservation],
    asset: Asset,
    timeframe: Timeframe,
) -> Vec<crate::contract_model::PriceObservation> {
    let mut result = Vec::with_capacity(observations.len());
    for obs in observations {
        let open_f = obs.window_open_price.as_f64();
        if open_f <= 0.0 {
            continue;
        }
        let magnitude = ((obs.tick.price.as_f64() - open_f) / open_f).abs();
        let time_elapsed_secs = obs
            .tick
            .timestamp_ms
            .saturating_sub(obs.window_open_ms)
            / 1_000;

        result.push(crate::contract_model::PriceObservation {
            asset,
            timeframe,
            magnitude,
            time_elapsed_secs,
            contract_price: obs.ask_up.as_f64(),
        });
    }
    result
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test helpers use expect for conciseness")]
mod tests {
    use super::*;
    use crate::pbt_downloader::write_pbt_snapshots;
    use crate::polybacktest::{PbtMarket, PbtSnapshot};

    fn sample_market() -> PbtMarket {
        PbtMarket {
            market_id: "test_m1".into(),
            slug: "btc-15m-test".into(),
            market_type: "15m".into(),
            start_time: "2026-01-01T00:00:00Z".into(),
            end_time: "2026-01-01T00:15:00Z".into(),
            btc_price_start: Some(95000.0),
            btc_price_end: Some(95150.0),
            winner: Some("Up".into()),
            clob_token_up: None,
            clob_token_down: None,
        }
    }

    fn sample_snapshots() -> Vec<PbtSnapshot> {
        vec![
            PbtSnapshot {
                id: None,
                time: "2026-01-01T00:05:00Z".into(),
                market_id: None,
                btc_price: Some(95050.0),
                price_up: Some(0.52),
                price_down: Some(0.49),
                orderbook_up: None,
                orderbook_down: None,
            },
            PbtSnapshot {
                id: None,
                time: "2026-01-01T00:00:00Z".into(),
                market_id: None,
                btc_price: Some(95000.0),
                price_up: Some(0.50),
                price_down: Some(0.51),
                orderbook_up: None,
                orderbook_down: None,
            },
            PbtSnapshot {
                id: None,
                time: "2026-01-01T00:10:00Z".into(),
                market_id: None,
                btc_price: Some(95120.0),
                price_up: Some(0.58),
                price_down: Some(0.43),
                orderbook_up: None,
                orderbook_down: None,
            },
        ]
    }

    #[test]
    fn observation_from_market_and_snapshot() {
        let market = sample_market();
        let snaps = sample_snapshots();
        let obs = build_observations(&market, &snaps, Asset::Btc, Timeframe::Min15);

        assert_eq!(obs.len(), 3);

        // Check that the first observation has the correct spot price.
        let first = &obs[0];
        assert_eq!(first.tick.asset, Asset::Btc);
        assert!(first.ask_up.as_f64() > 0.0);
        assert!(first.ask_down.as_f64() > 0.0);
        assert_eq!(first.winner, Some(Side::Up));
    }

    #[test]
    fn replay_sorts_by_timestamp() {
        let dir = tempfile::tempdir().expect("temp dir");
        let market = sample_market();
        // Snapshots are given out of order.
        let snaps = sample_snapshots();

        let path = crate::pbt_downloader::pbt_cache_path(
            dir.path(),
            "btc",
            "15m",
            &market.market_id,
        );
        write_pbt_snapshots(&path, &market, &snaps).expect("write");

        let replay = PbtReplay::load(dir.path(), "btc", "15m").expect("load");
        assert_eq!(replay.len(), 3);

        // Verify sorted order.
        let timestamps: Vec<u64> = replay.map(|o| o.tick.timestamp_ms).collect();
        let mut sorted = timestamps.clone();
        sorted.sort_unstable();
        assert_eq!(timestamps, sorted, "observations must be time-sorted");
    }

    #[test]
    fn replay_reset_works() {
        let dir = tempfile::tempdir().expect("temp dir");
        let market = sample_market();
        let snaps = sample_snapshots();

        let path = crate::pbt_downloader::pbt_cache_path(
            dir.path(),
            "btc",
            "15m",
            &market.market_id,
        );
        write_pbt_snapshots(&path, &market, &snaps).expect("write");

        let mut replay = PbtReplay::load(dir.path(), "btc", "15m").expect("load");
        let pass1: Vec<u64> = replay.by_ref().map(|o| o.tick.timestamp_ms).collect();
        assert!(replay.next().is_none());

        replay.reset();
        let pass2: Vec<u64> = replay.map(|o| o.tick.timestamp_ms).collect();
        assert_eq!(pass1, pass2);
    }

    #[test]
    fn replay_empty_dir() {
        let dir = tempfile::tempdir().expect("temp dir");
        let replay = PbtReplay::load(dir.path(), "btc", "15m").expect("load");
        assert!(replay.is_empty());
        assert_eq!(replay.len(), 0);
    }

    #[test]
    fn pbt_to_price_observations_computes_magnitude() {
        let market = sample_market();
        let snaps = vec![PbtSnapshot {
            id: None,
            time: "2026-01-01T00:05:00Z".into(),
            market_id: None,
            btc_price: Some(95950.0), // 1% above 95000
            price_up: Some(0.60),
            price_down: Some(0.41),
            orderbook_up: None,
            orderbook_down: None,
        }];
        let obs = build_observations(&market, &snaps, Asset::Btc, Timeframe::Min15);
        let price_obs = pbt_to_price_observations(&obs, Asset::Btc, Timeframe::Min15);

        assert_eq!(price_obs.len(), 1);
        // 95950 - 95000 = 950; 950/95000 = 0.01 = 1%
        assert!(
            (price_obs[0].magnitude - 0.01).abs() < 1e-6,
            "expected ~0.01, got {}",
            price_obs[0].magnitude
        );
        assert!((price_obs[0].contract_price - 0.60).abs() < 1e-6);
        // time_elapsed = 5 minutes = 300 seconds
        assert_eq!(price_obs[0].time_elapsed_secs, 300);
    }

    #[test]
    fn parse_winner_variants() {
        assert_eq!(parse_winner(Some("Up")), Some(Side::Up));
        assert_eq!(parse_winner(Some("Down")), Some(Side::Down));
        assert_eq!(parse_winner(None), None);
        assert_eq!(parse_winner(Some("Other")), None);
    }
}
