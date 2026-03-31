//! CLOB client initialization and order placement helpers.

use std::str::FromStr;
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::warn;
use alloy::signers::Signer as _;
use alloy::signers::local::PrivateKeySigner;
use polymarket_client_sdk::auth::Normal;
use polymarket_client_sdk::auth::state::Authenticated;
use polymarket_client_sdk::clob::types::{Amount, OrderType, Side as ClobSide};
use polymarket_client_sdk::clob::{Client, Config};
use polymarket_client_sdk::types::U256;
use polymarket_client_sdk::POLYGON;
use rust_decimal::Decimal;
use tracing::info;

/// Authenticated CLOB client + signer pair.
pub struct ClobContext {
    /// Authenticated Polymarket CLOB client.
    pub client: Client<Authenticated<Normal>>,
    /// Local wallet signer (Polygon chain).
    pub signer: Arc<PrivateKeySigner>,
}

/// Result of a successful market order fill.
#[derive(Debug, Clone)]
pub struct LiveFill {
    /// Order ID returned by the CLOB.
    pub order_id: String,
    /// Average fill price.
    pub avg_price: f64,
    /// USDC cost of the fill.
    pub cost_usdc: f64,
    /// Number of outcome shares received.
    pub shares: f64,
}

/// Initialize the CLOB client from `POLYMARKET_PRIVATE_KEY` env var.
///
/// Returns `Ok(None)` if the env var is not set (paper-only mode).
///
/// # Errors
///
/// Returns `Err` if the key is set but authentication fails.
pub async fn init_clob_client() -> Result<Option<ClobContext>> {
    let Ok(private_key) = std::env::var("POLYMARKET_PRIVATE_KEY") else {
        info!("POLYMARKET_PRIVATE_KEY not set -- live trading disabled");
        return Ok(None);
    };

    let signer = PrivateKeySigner::from_str(&private_key)
        .context("invalid POLYMARKET_PRIVATE_KEY")?
        .with_chain_id(Some(POLYGON));

    info!(address = %signer.address(), "CLOB signer initialized");

    let config = Config::builder()
        .heartbeat_interval(std::time::Duration::from_secs(5))
        .build();
    let client = Client::new("https://clob.polymarket.com", config)
        .context("failed to create CLOB client")?
        .authentication_builder(&signer)
        .signature_type(polymarket_client_sdk::clob::types::SignatureType::GnosisSafe)
        .authenticate()
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("CLOB authentication failed")?;

    info!("CLOB client authenticated successfully");

    Ok(Some(ClobContext {
        client,
        signer: Arc::new(signer),
    }))
}

/// Place a Fill-or-Kill market buy order with slippage protection.
///
/// `token_id` is the hex token ID string for the outcome token.
/// `size_usdc` is the USDC amount to spend.
/// `max_price` is the worst acceptable price per share (0.0-1.0).
/// The CLOB will reject the order if it cannot fill at or below this price.
///
/// # Errors
///
/// Returns `Err` if the order cannot be built, signed, posted, or is rejected.
pub async fn place_fok_order(
    ctx: &ClobContext,
    token_id: &str,
    side: ClobSide,
    size_usdc: f64,
    max_price: f64,
) -> Result<LiveFill> {
    // Convert the hex token_id string to U256.
    let token_u256 = U256::from_str(token_id)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("invalid token_id for U256 conversion")?;

    // Truncate to 2 decimal places for USDC precision.
    let size_dec = Decimal::try_from(size_usdc)
        .context("invalid size_usdc for Decimal conversion")?
        .round_dp(2);

    let amount = Amount::usdc(size_dec)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to create USDC Amount")?;

    // Worst-price slippage protection — the CLOB will reject the order
    // if the fill price would exceed this.
    let price_dec = Decimal::try_from(max_price)
        .context("invalid max_price for Decimal conversion")?
        .round_dp(2);

    let signable = ctx
        .client
        .market_order()
        .token_id(token_u256)
        .amount(amount)
        .price(price_dec)
        .side(side)
        .order_type(OrderType::FOK)
        .build()
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to build market FOK order")?;

    let signed = ctx
        .client
        .sign(&*ctx.signer, signable)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to sign order")?;

    let response = ctx
        .client
        .post_order(signed)
        .await
        .map_err(|e| {
            warn!(error = %e, "CLOB post_order raw error");
            anyhow::anyhow!("post_order: {e}")
        })?;

    if !response.success {
        anyhow::bail!(
            "order rejected: {}",
            response.error_msg.as_deref().unwrap_or("unknown")
        );
    }

    // Extract fill details from response.
    // making_amount = USDC spent, taking_amount = shares received (for a buy).
    let cost = response
        .making_amount
        .to_string()
        .parse::<f64>()
        .unwrap_or(size_usdc);
    let shares = response
        .taking_amount
        .to_string()
        .parse::<f64>()
        .unwrap_or(0.0);
    let avg_price = if shares > 0.0 { cost / shares } else { 0.0 };

    Ok(LiveFill {
        order_id: response.order_id,
        avg_price,
        cost_usdc: cost,
        shares,
    })
}

/// Result of posting a GTC order (no fill details — fills arrive via WS).
#[derive(Debug, Clone)]
pub struct GtcOrderResult {
    /// Order ID returned by the CLOB.
    pub order_id: String,
    /// Initial status: "Live", "Matched", etc.
    pub status: String,
}

/// Place a GTC (Good-Til-Cancelled) limit buy order.
///
/// If liquidity exists at our price, fills immediately as taker (~1.5% fee).
/// Otherwise rests on the book as maker (0% fee + rebates).
///
/// # Errors
///
/// Returns `Err` if the order cannot be built, signed, or posted.
pub async fn place_gtc_order(
    ctx: &ClobContext,
    token_id: &str,
    side: ClobSide,
    size_shares: f64,
    price: f64,
) -> Result<GtcOrderResult> {
    let token_u256 = U256::from_str(token_id)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("invalid token_id for U256 conversion")?;

    let price_dec = Decimal::try_from(price)
        .context("invalid price for Decimal conversion")?
        .round_dp(2);

    let size_dec = Decimal::try_from(size_shares)
        .context("invalid size for Decimal conversion")?
        .round_dp(2);

    let signable = ctx
        .client
        .limit_order()
        .token_id(token_u256)
        .price(price_dec)
        .size(size_dec)
        .side(side)
        .order_type(OrderType::GTC)
        // No post_only — fill as taker if crossing, rest as maker if not.
        .build()
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to build GTC limit order")?;

    let signed = ctx
        .client
        .sign(&*ctx.signer, signable)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to sign GTC order")?;

    let response = ctx
        .client
        .post_order(signed)
        .await
        .map_err(|e| {
            warn!(error = %e, "CLOB post_order (GTC) raw error");
            anyhow::anyhow!("post_order GTC: {e}")
        })?;

    if !response.success {
        anyhow::bail!(
            "GTC order rejected: {}",
            response.error_msg.as_deref().unwrap_or("unknown")
        );
    }

    Ok(GtcOrderResult {
        order_id: response.order_id,
        status: format!("{:?}", response.status),
    })
}

/// Cancel a resting order by ID.
///
/// # Errors
///
/// Returns `Err` if the cancel request fails.
pub async fn cancel_order(ctx: &ClobContext, order_id: &str) -> Result<()> {
    let response = ctx
        .client
        .cancel_order(order_id)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to cancel order")?;

    if !response.not_canceled.is_empty() {
        for (id, reason) in &response.not_canceled {
            warn!(order_id = %id, reason = %reason, "order cancel failed");
        }
    }

    Ok(())
}

/// Sell winning shares at $0.99 via a FOK market sell order.
///
/// After a market resolves in our favor, sell shares at near-$1 to
/// convert to USDC. This avoids the on-chain CTF redeem (which requires
/// the Safe/relayer) and nets ~$0.99 per share.
///
/// # Errors
///
/// Returns `Err` if the sell order fails.
pub async fn sell_winning_position(
    ctx: &ClobContext,
    token_id: &str,
    shares: f64,
) -> Result<f64> {
    let token_u256 = U256::from_str(token_id)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("invalid token_id for U256 conversion")?;

    // Sell shares — for SELL orders, amount is in shares (not USDC).
    let shares_dec = Decimal::try_from(shares)
        .context("invalid shares for Decimal conversion")?
        .round_dp(2);

    let amount = Amount::shares(shares_dec)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to create shares Amount")?;

    // Price floor: sell at minimum $0.99 per share.
    let price_dec = Decimal::try_from(0.99)
        .context("invalid price")?;

    let signable = ctx
        .client
        .market_order()
        .token_id(token_u256)
        .amount(amount)
        .price(price_dec)
        .side(ClobSide::Sell)
        .order_type(OrderType::FOK)
        .build()
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to build sell order")?;

    let signed = ctx
        .client
        .sign(&*ctx.signer, signable)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to sign sell order")?;

    let response = ctx
        .client
        .post_order(signed)
        .await
        .map_err(|e| {
            warn!(error = %e, "CLOB sell order raw error");
            anyhow::anyhow!("sell order: {e}")
        })?;

    if !response.success {
        anyhow::bail!(
            "sell order rejected: {}",
            response.error_msg.as_deref().unwrap_or("unknown")
        );
    }

    let usdc_received = response
        .taking_amount
        .to_string()
        .parse::<f64>()
        .unwrap_or(0.0);

    Ok(usdc_received)
}

/// Result of checking market resolution via the CLOB API.
#[derive(Debug, Clone)]
pub struct MarketResolutionResult {
    /// Whether the market has resolved.
    pub closed: bool,
    /// The winning token ID (empty if not yet resolved).
    pub winning_token_id: String,
}

/// Poll the CLOB API to check if a market has resolved.
///
/// Returns `Ok(result)` with `closed=true` and the winning token ID
/// when the market has resolved. Returns `closed=false` if still open.
///
/// # Errors
///
/// Returns `Err` if the HTTP request fails.
pub async fn check_market_resolution(
    http_client: &reqwest::Client,
    condition_id: &str,
) -> Result<MarketResolutionResult> {
    let url = format!("https://clob.polymarket.com/markets/{condition_id}");
    let response = http_client
        .get(&url)
        .send()
        .await
        .context("failed to fetch market status")?
        .text()
        .await
        .context("failed to read market response")?;

    // Parse the JSON response to extract `closed` and `tokens[].winner`.
    let v: serde_json::Value =
        serde_json::from_str(&response).context("failed to parse market JSON")?;

    let closed = v.get("closed").and_then(|c| c.as_bool()).unwrap_or(false);

    if !closed {
        return Ok(MarketResolutionResult {
            closed: false,
            winning_token_id: String::new(),
        });
    }

    // Find the winning token.
    let winning_token_id = v
        .get("tokens")
        .and_then(|t| t.as_array())
        .and_then(|tokens| {
            tokens.iter().find_map(|tok| {
                let winner = tok.get("winner").and_then(|w| w.as_bool()).unwrap_or(false);
                if winner {
                    tok.get("token_id")
                        .and_then(|id| id.as_str())
                        .map(String::from)
                } else {
                    None
                }
            })
        })
        .unwrap_or_default();

    Ok(MarketResolutionResult {
        closed: true,
        winning_token_id,
    })
}

/// Redeem winning tokens for USDC after market resolution.
///
/// Calls `redeemPositions` on the CTF contract with index_sets `[1, 2]`
/// (both YES and NO — only winners pay out).
///
/// # Errors
///
/// Returns `Err` if the on-chain transaction fails.
pub async fn redeem_winning_position(
    signer: &PrivateKeySigner,
    condition_id: &str,
) -> Result<String> {
    use alloy::providers::ProviderBuilder;
    use polymarket_client_sdk::ctf::Client as CtfClient;
    use polymarket_client_sdk::ctf::types::RedeemPositionsRequest;
    use polymarket_client_sdk::types::{Address, B256};

    let polygon_rpc = "https://polygon-rpc.com";

    // Build a provider with our signer for on-chain transactions.
    let provider = ProviderBuilder::new()
        .wallet(alloy::network::EthereumWallet::from(signer.clone()))
        .connect_http(polygon_rpc.parse().context("invalid RPC URL")?);

    let ctf_client = CtfClient::new(provider, POLYGON)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to create CTF client")?;

    // Parse condition_id from hex string to B256.
    let cond_b256 = B256::from_str(condition_id)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("invalid condition_id for B256")?;

    // USDC.e on Polygon.
    let usdc_address: Address = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174"
        .parse()
        .context("invalid USDC address")?;

    let request = RedeemPositionsRequest::for_binary_market(usdc_address, cond_b256);

    let response = ctf_client
        .redeem_positions(&request)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("redeem_positions transaction failed")?;

    let tx_hash = format!("{:?}", response.transaction_hash);
    info!(
        condition_id = %condition_id,
        tx_hash = %tx_hash,
        "REDEEM SUCCESS — winning tokens converted to USDC"
    );

    Ok(tx_hash)
}
