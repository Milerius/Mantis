//! Instrument records — canonical identity + venue bindings.

use mantis_types::{InstrumentId, InstrumentMeta};

use crate::bindings::{BinanceBinding, PolymarketBinding};
use crate::types::InstrumentKey;

/// A canonical instrument with its fixed-point conversion metadata.
#[derive(Clone, Debug)]
pub struct CanonicalInstrument<const D: u8> {
    /// Stable hot-path identity — never recycled.
    pub instrument_id: InstrumentId,
    /// Logical description (class, asset, timeframe, outcome).
    pub key: InstrumentKey,
    /// Tick/lot size for price/quantity conversion.
    pub meta: InstrumentMeta<D>,
}

/// Full instrument record: canonical identity + venue bindings.
#[derive(Clone, Debug)]
pub struct InstrumentRecord<const D: u8> {
    /// Stable identity + conversion metadata.
    pub canonical: CanonicalInstrument<D>,
    /// Binance binding (stable symbol).
    pub binance: Option<BinanceBinding>,
    /// Polymarket binding (rotating current/next windows).
    pub polymarket: Option<PolymarketBinding>,
}
