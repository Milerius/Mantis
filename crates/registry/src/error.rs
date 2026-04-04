//! Registry error types.

use mantis_types::InstrumentId;
use std::fmt;

/// Errors from registry operations.
#[derive(Debug)]
pub enum RegistryError {
    /// An instrument with this ID already exists.
    DuplicateInstrumentId(InstrumentId),
    /// An instrument with this key already exists.
    DuplicateInstrumentKey,
    /// A Binance symbol is already bound to another instrument.
    DuplicateBinanceSymbol(String),
    /// A Polymarket token ID is already bound to another instrument.
    DuplicatePolymarketTokenId(String),
    /// The requested instrument was not found.
    MissingInstrument(InstrumentId),
    /// The instrument has no Polymarket binding.
    MissingPolymarketBinding(InstrumentId),
    /// No next window to promote.
    NoNextWindow(InstrumentId),
    /// All 2^32 - 1 instrument IDs have been allocated.
    IdSpaceExhausted,
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateInstrumentId(id) => {
                write!(f, "duplicate instrument ID: {id}")
            }
            Self::DuplicateInstrumentKey => {
                write!(f, "duplicate instrument key")
            }
            Self::DuplicateBinanceSymbol(s) => {
                write!(f, "duplicate Binance symbol: {s}")
            }
            Self::DuplicatePolymarketTokenId(t) => {
                write!(
                    f,
                    "duplicate Polymarket token ID: {}",
                    &t[..t.len().min(20)]
                )
            }
            Self::MissingInstrument(id) => {
                write!(f, "instrument not found: {id}")
            }
            Self::MissingPolymarketBinding(id) => {
                write!(f, "no Polymarket binding for instrument: {id}")
            }
            Self::NoNextWindow(id) => {
                write!(f, "no next window to promote for instrument: {id}")
            }
            Self::IdSpaceExhausted => {
                write!(f, "instrument ID space exhausted (2^32 - 1 IDs used)")
            }
        }
    }
}

impl std::error::Error for RegistryError {}
