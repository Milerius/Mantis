//! Polymarket Gamma API scanner for active crypto Up/Down markets.
//!
//! Polls the Gamma REST API periodically to discover active binary prediction
//! markets for supported crypto assets. Results are used by [`crate::manager`]
//! to populate the live market map.

use chrono::Utc;
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

/// A single market entry nested inside a Gamma API event.
///
/// The real Gamma API encodes `clobTokenIds` and `outcomes` as JSON strings
/// (e.g. `"[\"id1\",\"id2\"]"`), so we deserialise them as raw strings and
/// parse them in [`extract_token_ids_from_strings`].
#[derive(Debug, Deserialize)]
pub struct GammaMarket {
    /// Market condition ID.
    #[serde(rename = "conditionId")]
    pub condition_id: String,
    /// Whether the market is currently active.
    #[serde(default)]
    pub active: bool,
    /// Whether the market is closed.
    #[serde(default)]
    pub closed: bool,
    /// JSON-encoded array of CLOB token IDs, e.g. `"[\"up_id\",\"down_id\"]"`.
    #[serde(rename = "clobTokenIds", default)]
    pub clob_token_ids: String,
    /// JSON-encoded array of outcome labels, e.g. `"[\"Up\",\"Down\"]"`.
    #[serde(default)]
    pub outcomes: String,
    /// End date/time ISO string.
    #[serde(rename = "endDate", default)]
    pub end_date: String,
}

/// A single event entry from the Gamma API response.
#[derive(Debug, Deserialize)]
pub struct GammaEvent {
    /// Event title (e.g. `"Bitcoin Up or Down - March 29, 12:15PM-12:30PM ET"`).
    pub title: String,
    /// Event slug (e.g. `"btc-updown-15m-1774798200"`).
    #[serde(default)]
    pub slug: String,
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
        // Default to 15-minute if the event is clearly crypto Up/Down but has no
        // explicit timeframe tag (the tag_slug=up-or-down endpoint returns mostly 15m).
        Timeframe::Min15
    };

    Some((asset, timeframe))
}

/// Extract `(token_id_up, token_id_down)` from the raw JSON-string fields
/// `clobTokenIds` and `outcomes` returned by the Gamma API.
///
/// The API encodes both as JSON arrays serialised into a string, e.g.:
/// - `clobTokenIds`: `"[\"tok_up\",\"tok_down\"]"`
/// - `outcomes`: `"[\"Up\",\"Down\"]"`
///
/// Returns `None` if either token cannot be resolved.
fn extract_token_ids_from_strings(
    clob_token_ids: &str,
    outcomes: &str,
) -> Option<(String, String)> {
    // Parse both JSON-string-encoded arrays.
    let ids: Vec<String> = serde_json::from_str(clob_token_ids).ok()?;
    let labels: Vec<String> = serde_json::from_str(outcomes).ok()?;

    if ids.len() < 2 || labels.len() < 2 {
        return None;
    }

    // Zip ids with outcome labels and find Up/Down.
    let mut up_id: Option<String> = None;
    let mut down_id: Option<String> = None;

    for (id, label) in ids.iter().zip(labels.iter()) {
        match label.to_lowercase().as_str() {
            "up" | "yes" => up_id = Some(id.clone()),
            "down" | "no" => down_id = Some(id.clone()),
            _ => {}
        }
    }

    // If label-based matching failed (e.g. labels are not "Up"/"Down"),
    // fall back to positional: first token = Up, second = Down.
    if up_id.is_none() || down_id.is_none() {
        return Some((ids[0].clone(), ids[1].clone()));
    }

    Some((up_id?, down_id?))
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Gamma API base URL.
pub const GAMMA_API_BASE: &str = "https://gamma-api.polymarket.com";

/// Scan the Polymarket Gamma API for active crypto Up/Down markets.
///
/// Queries `GET /events?limit=100&active=true&closed=false&tag_slug=up-or-down`
/// which targets the crypto Up/Down binary prediction markets specifically.
/// Each call typically takes 1–3 seconds over a low-latency connection.
///
/// # Errors
///
/// Returns [`ScanError::Http`] for transport failures and [`ScanError::Json`]
/// if the response cannot be parsed.
pub async fn scan_active_markets(
    client: &Client,
    assets: &[Asset],
) -> Result<Vec<MarketInfo>, ScanError> {
    let mut all_markets = Vec::new();

    // Query each timeframe separately — the `tag_slug=up-or-down` endpoint
    // without a timeframe tag returns stale markets from months ago.
    for tf_tag in &["5M", "15M"] {
        let url = format!(
            "{GAMMA_API_BASE}/events?limit=50&active=true&closed=false&tag_slug=up-or-down&tag_slug={tf_tag}"
        );
        debug!(url = %url, "scanning Gamma API");

        let response = client.get(&url).send().await?.text().await?;
        let markets = parse_gamma_response(&response, assets).map_err(ScanError::Json)?;
        all_markets.extend(markets);
    }

    Ok(all_markets)
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
        // Determine asset + timeframe from the event title and slug.
        let Some((asset, event_timeframe)) = parse_asset_timeframe(&event.title, &event.slug) else {
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

            // Skip markets whose window has already ended (with a 5-minute grace buffer
            // to tolerate slight resolution delays).  An empty end_date is treated as
            // unknown and passed through so we never silently drop markets the API
            // returns without a date.
            if !market.end_date.is_empty() {
                if let Ok(end_dt) = market.end_date.parse::<chrono::DateTime<Utc>>() {
                    let grace = chrono::Duration::minutes(5);
                    if end_dt + grace < Utc::now() {
                        debug!(
                            condition_id = %market.condition_id,
                            end_date = %market.end_date,
                            "skipping market — window already ended"
                        );
                        continue;
                    }
                }
            }

            let Some((token_id_up, token_id_down)) =
                extract_token_ids_from_strings(&market.clob_token_ids, &market.outcomes)
            else {
                debug!(
                    condition_id = %market.condition_id,
                    clob_token_ids = %market.clob_token_ids,
                    outcomes = %market.outcomes,
                    "skipping market — could not extract token IDs"
                );
                continue;
            };

            // Resolve timeframe from event slug/title (already parsed above).
            let timeframe = event_timeframe;

            results.push(MarketInfo {
                slug: event.slug.clone(),
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

    /// Sample response matching the real Gamma API shape for tag_slug=up-or-down.
    /// Dates are set far in the future so the end_time filter never drops them.
    const SAMPLE_GAMMA_RESPONSE: &str = r#"[
        {
            "title": "Bitcoin Up or Down - March 29, 12:15PM-12:30PM ET",
            "slug": "btc-updown-15m-1774798200",
            "markets": [
                {
                    "conditionId": "0xabc123",
                    "active": true,
                    "closed": false,
                    "endDate": "2030-01-15T12:15:00Z",
                    "clobTokenIds": "[\"tok_up_1\",\"tok_down_1\"]",
                    "outcomes": "[\"Up\",\"Down\"]"
                }
            ]
        },
        {
            "title": "Ethereum Up or Down - March 29, 1:00PM-2:00PM ET",
            "slug": "eth-updown-1h-1774800000",
            "markets": [
                {
                    "conditionId": "0xdef456",
                    "active": true,
                    "closed": false,
                    "endDate": "2030-01-15T13:00:00Z",
                    "clobTokenIds": "[\"tok_eth_up\",\"tok_eth_down\"]",
                    "outcomes": "[\"Up\",\"Down\"]"
                }
            ]
        },
        {
            "title": "Some Other Event",
            "slug": "some-other-event",
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
        assert_eq!(m.end_date, "2030-01-15T12:15:00Z");
    }

    #[test]
    fn parse_gamma_skips_closed_market() {
        let json = r#"[
            {
                "title": "Bitcoin Up or Down - March 29, 12:15PM-12:30PM ET",
                "slug": "btc-updown-15m-1774798200",
                "markets": [
                    {
                        "conditionId": "0x111",
                        "active": false,
                        "closed": true,
                        "endDate": "2024-01-01T00:00:00Z",
                        "clobTokenIds": "[\"t1\",\"t2\"]",
                        "outcomes": "[\"Up\",\"Down\"]"
                    }
                ]
            }
        ]"#;
        let results = parse_gamma_response(json, &[]).expect("parse should succeed");
        assert!(results.is_empty(), "closed market should be skipped");
    }

    #[test]
    fn parse_gamma_skips_expired_market() {
        // An active=true, closed=false market with an endDate well in the past
        // should be filtered by the end_time guard.
        let json = r#"[
            {
                "title": "Bitcoin Up or Down - March 29, 12:15PM-12:30PM ET",
                "slug": "btc-updown-15m-1774798200",
                "markets": [
                    {
                        "conditionId": "0xexpired",
                        "active": true,
                        "closed": false,
                        "endDate": "2020-01-01T00:00:00Z",
                        "clobTokenIds": "[\"tok_x\",\"tok_y\"]",
                        "outcomes": "[\"Up\",\"Down\"]"
                    }
                ]
            }
        ]"#;
        let results = parse_gamma_response(json, &[]).expect("parse should succeed");
        assert!(results.is_empty(), "expired market should be skipped by end_time filter");
    }

    #[test]
    fn parse_gamma_keeps_market_with_empty_end_date() {
        // An empty endDate should pass through (unknown — treat as active).
        let json = r#"[
            {
                "title": "Bitcoin Up or Down - March 29, 12:15PM-12:30PM ET",
                "slug": "btc-updown-15m-1774798200",
                "markets": [
                    {
                        "conditionId": "0xnodate",
                        "active": true,
                        "closed": false,
                        "endDate": "",
                        "clobTokenIds": "[\"tok_a\",\"tok_b\"]",
                        "outcomes": "[\"Up\",\"Down\"]"
                    }
                ]
            }
        ]"#;
        let results = parse_gamma_response(json, &[]).expect("parse should succeed");
        assert_eq!(results.len(), 1, "market with empty endDate should not be filtered");
    }

    #[test]
    fn parse_gamma_skips_market_without_token_ids() {
        let json = r#"[
            {
                "title": "Bitcoin Up or Down - March 29, 12:15PM-12:30PM ET",
                "slug": "btc-updown-15m-1774798200",
                "markets": [
                    {
                        "conditionId": "0x222",
                        "active": true,
                        "closed": false,
                        "endDate": "2030-01-15T13:00:00Z",
                        "clobTokenIds": "",
                        "outcomes": ""
                    }
                ]
            }
        ]"#;
        let results = parse_gamma_response(json, &[]).expect("parse should succeed");
        assert!(results.is_empty(), "market with empty token IDs should be skipped");
    }

    #[test]
    fn parse_gamma_positional_fallback_when_labels_unknown() {
        // When outcome labels are not "Up"/"Down", fall back to positional (first=Up, second=Down).
        let json = r#"[
            {
                "title": "Bitcoin Up or Down - March 29, 12:15PM-12:30PM ET",
                "slug": "btc-updown-15m-1774798200",
                "markets": [
                    {
                        "conditionId": "0x333",
                        "active": true,
                        "closed": false,
                        "endDate": "2030-01-15T12:15:00Z",
                        "clobTokenIds": "[\"tok_a\",\"tok_b\"]",
                        "outcomes": "[\"Yes\",\"No\"]"
                    }
                ]
            }
        ]"#;
        let results = parse_gamma_response(json, &[]).expect("parse should succeed");
        assert_eq!(results.len(), 1);
        // "Yes" maps to Up, "No" maps to Down.
        assert_eq!(results[0].token_id_up, "tok_a");
        assert_eq!(results[0].token_id_down, "tok_b");
    }

    #[test]
    fn parse_asset_timeframe_btc_15m() {
        let (asset, tf) = parse_asset_timeframe(
            "Bitcoin Up or Down - March 29, 12:15PM-12:30PM ET",
            "btc-updown-15m-1774798200",
        )
        .expect("should match BTC 15m");
        assert_eq!(asset, Asset::Btc);
        assert_eq!(tf, Timeframe::Min15);
    }

    #[test]
    fn parse_asset_timeframe_eth_1h() {
        let (asset, tf) = parse_asset_timeframe(
            "Ethereum Up or Down - March 29, 1:00PM-2:00PM ET",
            "eth-updown-1h-1774800000",
        )
        .expect("should match ETH 1h");
        assert_eq!(asset, Asset::Eth);
        assert_eq!(tf, Timeframe::Hour1);
    }

    #[test]
    fn parse_asset_timeframe_non_crypto_returns_none() {
        let result = parse_asset_timeframe("Gold Up or Down?", "gold-updown");
        assert!(result.is_none());
    }

    #[test]
    fn extract_token_ids_up_down_labels() {
        let (up, down) =
            extract_token_ids_from_strings(r#"["tok_up","tok_down"]"#, r#"["Up","Down"]"#)
                .expect("should extract tokens");
        assert_eq!(up, "tok_up");
        assert_eq!(down, "tok_down");
    }

    #[test]
    fn extract_token_ids_yes_no_labels() {
        let (up, down) =
            extract_token_ids_from_strings(r#"["tok_yes","tok_no"]"#, r#"["Yes","No"]"#)
                .expect("should extract tokens");
        assert_eq!(up, "tok_yes");
        assert_eq!(down, "tok_no");
    }

    #[test]
    fn extract_token_ids_positional_fallback() {
        // Unknown labels — falls back to positional.
        let (up, down) =
            extract_token_ids_from_strings(r#"["id_a","id_b"]"#, r#"["Foo","Bar"]"#)
                .expect("should extract tokens via positional fallback");
        assert_eq!(up, "id_a");
        assert_eq!(down, "id_b");
    }

    #[test]
    fn extract_token_ids_empty_string_returns_none() {
        let result = extract_token_ids_from_strings("", "");
        assert!(result.is_none(), "empty strings should return None");
    }
}
