//! PolyBackTest API client for downloading historical market snapshots.
//!
//! The PolyBackTest API (<https://api.polybacktest.com>) provides 8 snapshots/second
//! of historical orderbook data for Polymarket crypto Up/Down markets. This module
//! wraps the two main endpoints:
//!
//! - **List markets**: enumerate resolved markets by coin and market type.
//! - **Get snapshots**: fetch price snapshots (with optional orderbook depth) for a
//!   specific market.
//!
//! Authentication is via Bearer token passed in the `Authorization` header.

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::downloader::DownloadError;

// ─── API response types ─────────────────────────────────────────────────────

/// A market from the PolyBackTest API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PbtMarket {
    /// Unique market identifier.
    pub market_id: String,
    /// Human-readable slug (e.g. `"btc-15m-2026-01-01T00:00:00Z"`).
    pub slug: String,
    /// Market type: `"5m"`, `"15m"`, `"1h"`, `"4h"`, `"24h"`.
    pub market_type: String,
    /// ISO-8601 start time of the prediction window.
    pub start_time: String,
    /// ISO-8601 end time of the prediction window.
    pub end_time: String,
    /// Spot price of the underlying at window open.
    pub btc_price_start: Option<f64>,
    /// Spot price of the underlying at window close.
    pub btc_price_end: Option<f64>,
    /// Outcome: `"Up"`, `"Down"`, or `null` if unresolved.
    pub winner: Option<String>,
    /// CLOB token ID for the Up outcome.
    pub clob_token_up: Option<String>,
    /// CLOB token ID for the Down outcome.
    pub clob_token_down: Option<String>,
}

/// A snapshot from the PolyBackTest API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PbtSnapshot {
    /// Snapshot ID (unique).
    #[serde(default)]
    pub id: Option<u64>,
    /// ISO-8601 timestamp of this snapshot.
    pub time: String,
    /// Market ID this snapshot belongs to.
    #[serde(default)]
    pub market_id: Option<String>,
    /// Spot price of the underlying at snapshot time.
    ///
    /// Despite the field name (`btc_price`), this is the spot price for whatever
    /// coin the market tracks (BTC, ETH, or SOL). Can be `null` for very recent
    /// or incomplete snapshots.
    pub btc_price: Option<f64>,
    /// Best ask price for the Up contract. Can be `null` for empty orderbooks.
    pub price_up: Option<f64>,
    /// Best ask price for the Down contract. Can be `null` for empty orderbooks.
    pub price_down: Option<f64>,
}

/// Response wrapper for `GET /v2/markets`.
#[derive(Debug, Deserialize)]
struct MarketsResponse {
    markets: Vec<PbtMarket>,
    #[serde(default)]
    #[expect(dead_code, reason = "pagination field")]
    total: Option<u64>,
    #[serde(default)]
    #[expect(dead_code, reason = "pagination field")]
    limit: Option<u64>,
    #[serde(default)]
    #[expect(dead_code, reason = "pagination field")]
    offset: Option<u64>,
    #[serde(default)]
    #[expect(dead_code, reason = "API warning field")]
    warning: Option<String>,
}

/// Response wrapper for `GET /v2/markets/{market_id}/snapshots`.
///
/// The API also returns `total`, `limit`, `offset` pagination fields which we
/// ignore via `serde(default)`.
#[derive(Debug, Deserialize)]
struct SnapshotsResponse {
    #[expect(dead_code, reason = "market metadata included in response but not consumed here")]
    market: serde_json::Value,
    snapshots: Vec<PbtSnapshot>,
    #[serde(default)]
    #[expect(dead_code, reason = "pagination field")]
    total: Option<u64>,
    #[serde(default)]
    #[expect(dead_code, reason = "pagination field")]
    limit: Option<u64>,
    #[serde(default)]
    #[expect(dead_code, reason = "pagination field")]
    offset: Option<u64>,
}

// ─── PbtClient ──────────────────────────────────────────────────────────────

/// Base URL for the PolyBackTest API.
const BASE_URL: &str = "https://api.polybacktest.com";

/// Client for the PolyBackTest API.
///
/// Wraps a [`reqwest::Client`] and appends the Bearer token to every request.
pub struct PbtClient {
    client: Client,
    api_key: String,
}

impl PbtClient {
    /// Create a new client with the given API key.
    #[must_use]
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
        }
    }

    /// Create a new client re-using an existing [`reqwest::Client`].
    #[must_use]
    pub fn with_client(client: Client, api_key: String) -> Self {
        Self { client, api_key }
    }

    /// List resolved markets for a coin and market type.
    ///
    /// # Parameters
    ///
    /// - `coin` — `"btc"`, `"eth"`, or `"sol"`.
    /// - `market_type` — `"5m"`, `"15m"`, `"1h"`, `"4h"`, `"24h"`.
    /// - `limit` — maximum number of markets to return (API max is typically 100).
    /// - `offset` — pagination offset.
    ///
    /// # Errors
    ///
    /// Returns [`DownloadError`] on HTTP or parse failures.
    pub async fn list_markets(
        &self,
        coin: &str,
        market_type: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<PbtMarket>, DownloadError> {
        let url = format!(
            "{BASE_URL}/v2/markets?coin={coin}&market_type={market_type}&limit={limit}&offset={offset}"
        );

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp
                .text()
                .await
                .unwrap_or_else(|_| String::from("<unreadable>"));
            return Err(DownloadError::Api(format!(
                "PBT list_markets HTTP {status}: {body}"
            )));
        }

        let parsed: MarketsResponse = resp.json().await.map_err(DownloadError::Json)?;
        Ok(parsed.markets)
    }

    /// List **all** resolved markets by paginating until the API returns an empty page.
    ///
    /// # Errors
    ///
    /// Returns [`DownloadError`] on any HTTP or parse failure.
    pub async fn list_all_markets(
        &self,
        coin: &str,
        market_type: &str,
    ) -> Result<Vec<PbtMarket>, DownloadError> {
        let mut all = Vec::new();
        let mut offset: u32 = 0;
        let page_size: u32 = 100;

        loop {
            // Rate limit: 300 req/min, burst max 10/sec → 250ms between requests.
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;

            let page = self
                .list_markets(coin, market_type, page_size, offset)
                .await?;
            let count = page.len();
            all.extend(page);
            if (count as u32) < page_size {
                break;
            }
            offset += page_size;
        }
        Ok(all)
    }

    /// Get snapshots for a market (without orderbook depth).
    ///
    /// # Parameters
    ///
    /// - `market_id` — the unique market identifier from [`PbtMarket::market_id`].
    /// - `coin` — `"btc"`, `"eth"`, or `"sol"`.
    /// - `limit` — maximum snapshots per page (up to 1000).
    /// - `start_time` — optional ISO-8601 start time for pagination.
    ///
    /// # Errors
    ///
    /// Returns [`DownloadError`] on HTTP or parse failures.
    pub async fn get_snapshots(
        &self,
        market_id: &str,
        coin: &str,
        limit: u32,
        start_time: Option<&str>,
    ) -> Result<Vec<PbtSnapshot>, DownloadError> {
        let mut url = format!(
            "{BASE_URL}/v2/markets/{market_id}/snapshots?coin={coin}&limit={limit}&include_orderbook=false"
        );
        if let Some(st) = start_time {
            url.push_str(&format!("&start_time={st}"));
        }

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp
                .text()
                .await
                .unwrap_or_else(|_| String::from("<unreadable>"));
            return Err(DownloadError::Api(format!(
                "PBT get_snapshots HTTP {status}: {body}"
            )));
        }

        let body = resp.text().await?;
        let parsed: SnapshotsResponse = serde_json::from_str(&body).map_err(|e| {
            tracing::debug!(
                market_id,
                error = %e,
                body_len = body.len(),
                body_prefix = &body[..body.len().min(300)],
                "snapshot parse failure detail"
            );
            DownloadError::Api(format!("json parse: {e}"))
        })?;
        Ok(parsed.snapshots)
    }

    /// Get **all** snapshots for a market by paginating.
    ///
    /// Uses the last snapshot's `time` field as `start_time` for the next page.
    ///
    /// # Errors
    ///
    /// Returns [`DownloadError`] on any HTTP or parse failure.
    pub async fn get_all_snapshots(
        &self,
        market_id: &str,
        coin: &str,
    ) -> Result<Vec<PbtSnapshot>, DownloadError> {
        let mut all = Vec::new();
        let page_size: u32 = 1000;
        let mut cursor: Option<String> = None;

        loop {
            // Rate limit: 300 req/min, burst max 10/sec → 250ms between requests.
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;

            let page = self
                .get_snapshots(market_id, coin, page_size, cursor.as_deref())
                .await?;
            let count = page.len();
            if let Some(last) = page.last() {
                cursor = Some(last.time.clone());
            }
            all.extend(page);
            if (count as u32) < page_size {
                break;
            }
        }
        Ok(all)
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pbt_market_json() {
        let json = r#"{
            "market_id": "abc123",
            "slug": "btc-15m-2026-01-01",
            "market_type": "15m",
            "start_time": "2026-01-01T00:00:00Z",
            "end_time": "2026-01-01T00:15:00Z",
            "btc_price_start": 95000.0,
            "btc_price_end": 95150.0,
            "winner": "Up",
            "clob_token_up": "token_up_123",
            "clob_token_down": "token_down_456"
        }"#;
        let market: PbtMarket = serde_json::from_str(json).expect("should parse market JSON");
        assert_eq!(market.market_id, "abc123");
        assert_eq!(market.market_type, "15m");
        assert_eq!(market.winner, Some("Up".to_string()));
        assert!((market.btc_price_start.expect("start price") - 95000.0).abs() < 1e-6);
    }

    #[test]
    fn parse_pbt_market_null_winner() {
        let json = r#"{
            "market_id": "def456",
            "slug": "eth-5m-2026-01-01",
            "market_type": "5m",
            "start_time": "2026-01-01T00:00:00Z",
            "end_time": "2026-01-01T00:05:00Z",
            "btc_price_start": null,
            "btc_price_end": null,
            "winner": null,
            "clob_token_up": null,
            "clob_token_down": null
        }"#;
        let market: PbtMarket = serde_json::from_str(json).expect("should parse null-winner market");
        assert!(market.winner.is_none());
        assert!(market.btc_price_start.is_none());
    }

    #[test]
    fn parse_pbt_snapshot_json() {
        let json = r#"{
            "time": "2026-01-01T00:00:00.125Z",
            "btc_price": 95000.5,
            "price_up": 0.52,
            "price_down": 0.49
        }"#;
        let snap: PbtSnapshot = serde_json::from_str(json).expect("should parse snapshot JSON");
        assert_eq!(snap.time, "2026-01-01T00:00:00.125Z");
        assert!((snap.btc_price - 95000.5).abs() < 1e-6);
        assert!((snap.price_up - 0.52).abs() < 1e-6);
        assert!((snap.price_down - 0.49).abs() < 1e-6);
    }

    #[test]
    fn parse_markets_response() {
        let json = r#"{
            "markets": [
                {
                    "market_id": "m1",
                    "slug": "btc-15m",
                    "market_type": "15m",
                    "start_time": "2026-01-01T00:00:00Z",
                    "end_time": "2026-01-01T00:15:00Z",
                    "btc_price_start": 95000.0,
                    "btc_price_end": 95100.0,
                    "winner": "Up",
                    "clob_token_up": null,
                    "clob_token_down": null
                }
            ]
        }"#;
        let resp: MarketsResponse = serde_json::from_str(json).expect("should parse response");
        assert_eq!(resp.markets.len(), 1);
        assert_eq!(resp.markets[0].market_id, "m1");
    }

    #[test]
    fn pbt_client_new() {
        let client = PbtClient::new("test_key".to_string());
        assert_eq!(client.api_key, "test_key");
    }
}
