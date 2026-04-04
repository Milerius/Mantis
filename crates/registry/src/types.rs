//! Domain types for instrument classification.

/// Instrument class, what kind of product is this.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InstrumentClass {
    /// Spot market ( BTC/USDT).
    Spot,
    /// Perpetual futures.
    Perp,
    /// Binary prediction market ( Polymarket Up/Down).
    PredictionBinary,
    /// Oracle reference feed (Chainlink BTC/USD).
    OracleReference,
}

/// Base or quote asset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[expect(missing_docs)]
pub enum Asset {
    Btc,
    Eth,
    Sol,
    Xrp,
    Doge,
    Usdt,
    Usdc,
    Usd,
}

/// Timeframe for recurring markets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Timeframe {
    /// 5-minute window.
    M5,
    /// 15-minute window.
    M15,
    /// 1-hour window.
    M60,
    /// 4-hour window.
    H4,
}

/// Outcome side for binary prediction markets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[expect(missing_docs)]
pub enum OutcomeSide {
    Up,
    Down,
    Yes,
    No,
}

/// Stable logical description of an instrument.
///
/// This is NOT a hot-path type it's used in the registry and config layer
/// to identify instruments by their logical meaning. The hot path only sees
/// [`mantis_types::InstrumentId`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[expect(missing_docs)]
pub struct InstrumentKey {
    pub class: InstrumentClass,
    pub base: Asset,
    pub quote: Option<Asset>,
    pub timeframe: Option<Timeframe>,
    pub outcome: Option<OutcomeSide>,
}

impl InstrumentKey {
    /// Create a Polymarket binary prediction key.
    #[must_use]
    pub fn prediction(base: Asset, timeframe: Timeframe, outcome: OutcomeSide) -> Self {
        Self {
            class: InstrumentClass::PredictionBinary,
            base,
            quote: Some(Asset::Usdc),
            timeframe: Some(timeframe),
            outcome: Some(outcome),
        }
    }

    /// Create a Binance spot reference key.
    #[must_use]
    pub fn spot_reference(base: Asset, quote: Asset) -> Self {
        Self {
            class: InstrumentClass::OracleReference,
            base,
            quote: Some(quote),
            timeframe: None,
            outcome: None,
        }
    }

    /// Human-readable symbol ("BTC-M15-Up").
    #[must_use]
    pub fn symbol(&self) -> String {
        let base = format!("{:?}", self.base).to_uppercase();
        let mut parts = vec![base];

        if let Some(tf) = self.timeframe {
            parts.push(format!("{tf:?}"));
        }
        if let Some(outcome) = self.outcome {
            parts.push(format!("{outcome:?}"));
        }
        if self.class == InstrumentClass::OracleReference {
            if let Some(quote) = self.quote {
                parts.push(format!("{quote:?}").to_uppercase());
            }
            parts.push("Ref".to_owned());
        }

        parts.join("-")
    }
}
