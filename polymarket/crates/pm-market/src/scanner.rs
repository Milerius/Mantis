//! Polymarket Gamma API scanner for active crypto Up/Down markets.
//!
//! Polls the Gamma REST API periodically to discover active binary prediction
//! markets for supported crypto assets. Results are used by [`crate::manager`]
//! to populate the live market map.

use pm_types::{Asset, Timeframe};
use reqwest::Client;
use serde::Deserialize;
use thiserror::Error;
use tracing::debug;

// ─── Error ───────────────────────────────────────────────────────────────────

/// Errors produced by the Gamma API scanner.
#[derive(Debug, Error)]
pub enum ScanError {
    /// HTTP transport or server error.
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    /// JSON deserialisation failure.
    #[error("json parse error: {0}")]
    Json(serde_json::Error),
}

// ─── Public types ────────────────────────────────────────────────────────────

/// Discovered market info from a Gamma API event.
#[derive(Debug, Clone)]
pub struct MarketInfo {
    /// Market slug (e.g. `"btc-updown-15m-2024-01-15T12:00:00Z"`).
    pub slug: String,
    /// Polymarket condition ID (hex string).
    pub condition_id: String,
    /// Token ID for the Up (YES) outcome.
    pub token_id_up: String,
    /// Token ID for the Down (NO) outcome.
    pub token_id_down: String,
    /// Underlying crypto asset.
    pub asset: Asset,
    /// Prediction window timeframe.
    pub timeframe: Timeframe,
    /// ISO-8601 end date/time string from the API.
    pub end_date: String,
}

// ─── Gamma API response types ────────────────────────────────────────────────

/// A single token entry from the Gamma API `tokens` array.
#[derive(Debug, Deserialize)]
pub struct GammaToken {
    /// Token ID string.
    #[serde(rename = "token_id")]
    pub token_id: String,
    /// Outcome label (e.g. `"Up"`, `"Down"`).
    pub outcome: String,
}

/// A single market entry nested inside a Gamma API event.
#[derive(Debug, Deserialize)]
pub struct GammaMarket {
    /// Market condition ID.
    #[serde(rename = "conditionId")]
    pub condition_id: String,
    /// Market slug.
    pub slug: String,
    /// Whether the market is currently active.
    #[serde(default)]
    pub active: bool,
    /// Whether the market is closed.
    #[serde(default)]
    pub closed: bool,
    /// Tokens with their outcome labels.
    #[serde(default)]
    pub tokens: Vec<GammaToken>,
    /// End date/time ISO string.
    #[serde(rename = "endDate", default)]
    pub end_date: String,
}

/// A single event entry from the Gamma API response.
#[derive(Debug, Deserialize)]
pub struct GammaEvent {
    /// Event title (e.g. `"Bitcoin Up or Down?"`).
    pub title: String,
    /// Markets contained in this event.
    #[serde(default)]
    pub markets: Vec<GammaMarket>,
}

// ─── Parsing helpers ─────────────────────────────────────────────────────────

/// Attempt to identify the asset and timeframe from a market slug or event title.
///
/// Returns `None` if the text does not match a known Up/Down pattern.
fn parse_asset_timeframe(title: &str, slug: &str) -> Option<(Asset, Timeframe)> {
    let combined = format!("{title} {slug}").to_lowercase();

    let asset = if combined.contains("bitcoin") || combined.contains("btc") {
        Asset::Btc
    } else if combined.contains("ethereum") || combined.contains("eth") {
        Asset::Eth
    } else if combined.contains("solana") || combined.contains("sol") {
        Asset::Sol
    } else if combined.contains("xrp") || combined.contains("ripple") {
        Asset::Xrp
    } else {
        return None;
    };

    // Must be an Up/Down market.
    if !combined.contains("up") && !combined.contains("down") && !combined.contains("updown") {
        return None;
    }

    let timeframe = if combined.contains("15m") || combined.contains("15-min") {
        Timeframe::Min15
    } else if combined.contains("5m") || combined.contains("5-min") {
        Timeframe::Min5
    } else if combined.contains("4h") || combined.contains("4-hour") {
        Timeframe::Hour4
    } else if combined.contains("1h") || combined.contains("1-hour") || combined.contains("60m") {
        Timeframe::Hour1
    } else if combined.contains("hour") {
        // Generic "hour" match falls back to 1-hour.
        Timeframe::Hour1
    } else {
        // Default to 1-hour if the event is clearly crypto Up/Down but has no
        // explicit timeframe tag.
        Timeframe::Hour1
    };

    Some((asset, timeframe))
}

/// Extract `(token_id_up, token_id_down)` from the tokens array.
///
/// Returns `None` if both Up and Down tokens cannot be found.
fn extract_token_ids(tokens: &[GammaToken]) -> Option<(String, String)> {
    let mut up_id: Option<String> = None;
    let mut down_id: Option<String> = None;

    for token in tokens {
        match token.outcome.to_lowercase().as_str() {
            "up" | "yes" => up_id = Some(token.token_id.clone()),
            "down" | "no" => down_id = Some(token.token_id.clone()),
            _ => {}
        }
    }

    match (up_id, down_id) {
        (Some(up), Some(down)) => Some((up, down)),
        _ => None,
    }
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Gamma API base URL.
pub const GAMMA_API_BASE: &str = "https://gamma-api.polymarket.com";

/// Scan the Polymarket Gamma API for active crypto Up/Down markets.
///
/// Queries `GET /events?limit=50&active=true&closed=false` and filters the
/// results to markets matching the requested `assets`. Each call typically
/// takes 1–3 seconds over a low-latency connection.
///
/// # Errors
///
/// Returns [`ScanError::Http`] for transport failures and [`ScanError::Json`]
/// if the response cannot be parsed.
pub async fn scan_active_markets(
    client: &Client,
    assets: &[Asset],
) -> Result<Vec<MarketInfo>, ScanError> {
    let url = format!("{GAMMA_API_BASE}/events?limit=50&active=true&closed=false");
    debug!(url = %url, "scanning Gamma API for active markets");

    let response = client.get(&url).send().await?.text().await?;
    parse_gamma_response(&response, assets).map_err(ScanError::Json)
}

/// Parse a raw Gamma API JSON response string into [`MarketInfo`] entries.
///
/// Exposed for unit testing without live network calls.
///
/// # Errors
///
/// Returns a [`serde_json::Error`] if `json` cannot be deserialised.
pub fn parse_gamma_response(json: &str, assets: &[Asset]) -> Result<Vec<MarketInfo>, serde_json::Error> {
    let events: Vec<GammaEvent> = serde_json::from_str(json)?;
    let mut results = Vec::new();

    for event in &events {
        // Determine asset from the event title (e.g. "Bitcoin Up or Down?").
        // Pass an empty slug here; the timeframe will be resolved per-market.
        let Some((asset, _event_timeframe)) = parse_asset_timeframe(&event.title, "") else {
            continue;
        };

        // Filter to requested assets only.
        if !assets.is_empty() && !assets.contains(&asset) {
            continue;
        }

        for market in &event.markets {
            if !market.active || market.closed {
                continue;
            }

            let Some((token_id_up, token_id_down)) = extract_token_ids(&market.tokens) else {
                continue;
            };

            // Resolve timeframe from the market slug (most specific source).
            // Fall back to the event title if the slug has no timeframe tag.
            let (_, timeframe) = parse_asset_timeframe(&event.title, &market.slug)
                .unwrap_or((asset, _event_timeframe));

            results.push(MarketInfo {
                slug: market.slug.clone(),
                condition_id: market.condition_id.clone(),
                token_id_up,
                token_id_down,
                asset,
                timeframe,
                end_date: market.end_date.clone(),
            });

            debug!(
                condition_id = %market.condition_id,
                asset = %asset,
                timeframe = %timeframe,
                "discovered active market"
            );
        }
    }

    Ok(results)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_GAMMA_RESPONSE: &str = r#"[
        {
            "title": "Bitcoin Up or Down?",
            "markets": [
                {
                    "conditionId": "0xabc123",
                    "slug": "btc-updown-15m-2024-01-15",
                    "active": true,
                    "closed": false,
                    "endDate": "2024-01-15T12:15:00Z",
                    "tokens": [
                        {"token_id": "tok_up_1", "outcome": "Up"},
                        {"token_id": "tok_down_1", "outcome": "Down"}
                    ]
                }
            ]
        },
        {
            "title": "Ethereum Up or Down?",
            "markets": [
                {
                    "conditionId": "0xdef456",
                    "slug": "eth-updown-1h-2024-01-15",
                    "active": true,
                    "closed": false,
                    "endDate": "2024-01-15T13:00:00Z",
                    "tokens": [
                        {"token_id": "tok_eth_up", "outcome": "Up"},
                        {"token_id": "tok_eth_down", "outcome": "Down"}
                    ]
                }
            ]
        },
        {
            "title": "Some Other Event",
            "markets": []
        }
    ]"#;

    #[test]
    fn parse_gamma_returns_crypto_markets() {
        let results = parse_gamma_response(SAMPLE_GAMMA_RESPONSE, &[])
            .expect("parse should succeed");
        assert_eq!(results.len(), 2, "expected 2 crypto Up/Down markets");
    }

    #[test]
    fn parse_gamma_filters_by_asset() {
        let results = parse_gamma_response(SAMPLE_GAMMA_RESPONSE, &[Asset::Btc])
            .expect("parse should succeed");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].asset, Asset::Btc);
    }

    #[test]
    fn parse_gamma_btc_market_fields() {
        let results = parse_gamma_response(SAMPLE_GAMMA_RESPONSE, &[Asset::Btc])
            .expect("parse should succeed");
        let m = &results[0];
        assert_eq!(m.condition_id, "0xabc123");
        assert_eq!(m.token_id_up, "tok_up_1");
        assert_eq!(m.token_id_down, "tok_down_1");
        assert_eq!(m.timeframe, Timeframe::Min15);
        assert_eq!(m.end_date, "2024-01-15T12:15:00Z");
    }

    #[test]
    fn parse_gamma_skips_closed_market() {
        let json = r#"[
            {
                "title": "Bitcoin Up or Down?",
                "markets": [
                    {
                        "conditionId": "0x111",
                        "slug": "btc-updown-1h",
                        "active": false,
                        "closed": true,
                        "endDate": "2024-01-01T00:00:00Z",
                        "tokens": [
                            {"token_id": "t1", "outcome": "Up"},
                            {"token_id": "t2", "outcome": "Down"}
                        ]
                    }
                ]
            }
        ]"#;
        let results = parse_gamma_response(json, &[]).expect("parse should succeed");
        assert!(results.is_empty(), "closed market should be skipped");
    }

    #[test]
    fn parse_gamma_skips_market_without_both_tokens() {
        let json = r#"[
            {
                "title": "Bitcoin Up or Down?",
                "markets": [
                    {
                        "conditionId": "0x222",
                        "slug": "btc-updown-1h",
                        "active": true,
                        "closed": false,
                        "endDate": "2024-01-15T13:00:00Z",
                        "tokens": [
                            {"token_id": "t1", "outcome": "Up"}
                        ]
                    }
                ]
            }
        ]"#;
        let results = parse_gamma_response(json, &[]).expect("parse should succeed");
        assert!(results.is_empty(), "market without Down token should be skipped");
    }

    #[test]
    fn parse_asset_timeframe_btc_15m() {
        let (asset, tf) = parse_asset_timeframe("Bitcoin Up or Down?", "btc-updown-15m")
            .expect("should match BTC 15m");
        assert_eq!(asset, Asset::Btc);
        assert_eq!(tf, Timeframe::Min15);
    }

    #[test]
    fn parse_asset_timeframe_eth_1h() {
        let (asset, tf) = parse_asset_timeframe("Ethereum Up or Down?", "eth-updown-1h")
            .expect("should match ETH 1h");
        assert_eq!(asset, Asset::Eth);
        assert_eq!(tf, Timeframe::Hour1);
    }

    #[test]
    fn parse_asset_timeframe_non_crypto_returns_none() {
        let result = parse_asset_timeframe("Gold Up or Down?", "gold-updown");
        assert!(result.is_none());
    }
}
