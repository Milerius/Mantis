//! Asset, Side, Timeframe, and `ExchangeSource` enums for Polymarket trading.
//!
//! These enums represent the domain vocabulary used throughout the bot.
//! Each enum is `Copy + Clone` and includes utilities required by downstream
//! crates (indexing, symbol strings, duration helpers, etc.).

#[cfg(feature = "std")]
use serde::{Deserialize, Serialize};

// ─── Asset ───────────────────────────────────────────────────────────────────

/// Supported underlying crypto assets tracked by the bot.
///
/// The ordinal produced by [`Asset::index`] is stable and matches the order
/// variants are declared; downstream crates may use it to index fixed-size
/// arrays of size [`Asset::COUNT`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "std", serde(rename_all = "lowercase"))]
pub enum Asset {
    /// Bitcoin.
    Btc,
    /// Ethereum.
    Eth,
    /// Solana.
    Sol,
    /// XRP (Ripple).
    Xrp,
}

impl Asset {
    /// Number of variants in this enum.
    pub const COUNT: usize = 4;

    /// All variants in declaration order.
    pub const ALL: [Self; Self::COUNT] = [Self::Btc, Self::Eth, Self::Sol, Self::Xrp];

    /// Stable zero-based index for this variant.
    ///
    /// Suitable for indexing into `[T; Asset::COUNT]` arrays.
    #[inline]
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Btc => 0,
            Self::Eth => 1,
            Self::Sol => 2,
            Self::Xrp => 3,
        }
    }

    /// Binance spot symbol (e.g. `"BTCUSDT"`).
    #[inline]
    #[must_use]
    pub const fn binance_symbol(self) -> &'static str {
        match self {
            Self::Btc => "BTCUSDT",
            Self::Eth => "ETHUSDT",
            Self::Sol => "SOLUSDT",
            Self::Xrp => "XRPUSDT",
        }
    }

    /// OKX instrument ID for the perpetual swap (e.g. `"BTC-USDT-SWAP"`).
    #[inline]
    #[must_use]
    pub const fn okx_inst_id(self) -> &'static str {
        match self {
            Self::Btc => "BTC-USDT-SWAP",
            Self::Eth => "ETH-USDT-SWAP",
            Self::Sol => "SOL-USDT-SWAP",
            Self::Xrp => "XRP-USDT-SWAP",
        }
    }

    /// Lowercase ASCII name for this asset (e.g. `"btc"`).
    ///
    /// Returns a `'static` str — never allocates. Use instead of
    /// `asset.to_string().to_lowercase()` in hot paths.
    #[inline]
    #[must_use]
    pub const fn as_lower_str(self) -> &'static str {
        match self {
            Self::Btc => "btc",
            Self::Eth => "eth",
            Self::Sol => "sol",
            Self::Xrp => "xrp",
        }
    }
}

impl core::fmt::Display for Asset {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Btc => write!(f, "BTC"),
            Self::Eth => write!(f, "ETH"),
            Self::Sol => write!(f, "SOL"),
            Self::Xrp => write!(f, "XRP"),
        }
    }
}

// ─── Side ────────────────────────────────────────────────────────────────────

/// Direction of a binary Polymarket outcome relative to the current price.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub enum Side {
    /// The price will be higher at expiry.
    Up,
    /// The price will be lower at expiry.
    Down,
}

impl Side {
    /// Return the opposite side.
    ///
    /// This operation is its own inverse: `side.opposite().opposite() == side`.
    #[inline]
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Up => Self::Down,
            Self::Down => Self::Up,
        }
    }
}

impl core::fmt::Display for Side {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Up => write!(f, "Up"),
            Self::Down => write!(f, "Down"),
        }
    }
}

// ─── Timeframe ───────────────────────────────────────────────────────────────

/// Candle timeframe / prediction window length.
///
/// The ordinal produced by [`Timeframe::index`] is stable; downstream crates
/// may use it to index fixed-size arrays of size [`Timeframe::COUNT`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "std", serde(rename_all = "lowercase"))]
pub enum Timeframe {
    /// 5-minute candle / window.
    Min5,
    /// 15-minute candle / window.
    Min15,
    /// 1-hour candle / window.
    Hour1,
    /// 4-hour candle / window.
    Hour4,
}

impl Timeframe {
    /// Number of variants in this enum.
    pub const COUNT: usize = 4;

    /// All variants in declaration order.
    pub const ALL: [Self; Self::COUNT] = [Self::Min5, Self::Min15, Self::Hour1, Self::Hour4];

    /// Stable zero-based index for this variant.
    #[inline]
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Min5 => 0,
            Self::Min15 => 1,
            Self::Hour1 => 2,
            Self::Hour4 => 3,
        }
    }

    /// Duration of this timeframe in seconds.
    #[inline]
    #[must_use]
    pub const fn duration_secs(self) -> u64 {
        match self {
            Self::Min5 => 5 * 60,
            Self::Min15 => 15 * 60,
            Self::Hour1 => 60 * 60,
            Self::Hour4 => 4 * 60 * 60,
        }
    }

    /// Short label for this timeframe (e.g. `"5m"`, `"1h"`).
    ///
    /// Returns a `'static` str — never allocates. Use instead of
    /// `format!("{timeframe}")` in hot paths.
    #[inline]
    #[must_use]
    pub const fn as_label(self) -> &'static str {
        match self {
            Self::Min5 => "5m",
            Self::Min15 => "15m",
            Self::Hour1 => "1h",
            Self::Hour4 => "4h",
        }
    }
}

impl core::fmt::Display for Timeframe {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Min5 => write!(f, "5m"),
            Self::Min15 => write!(f, "15m"),
            Self::Hour1 => write!(f, "1h"),
            Self::Hour4 => write!(f, "4h"),
        }
    }
}

// ─── ExchangeSource ──────────────────────────────────────────────────────────

/// The exchange that produced a price tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub enum ExchangeSource {
    /// Binance spot / futures feed.
    Binance,
    /// OKX perpetual swap feed.
    Okx,
}

impl core::fmt::Display for ExchangeSource {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Binance => write!(f, "Binance"),
            Self::Okx => write!(f, "OKX"),
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    extern crate std;
    use std::string::ToString;

    use super::*;

    // Asset index uniqueness

    #[test]
    fn asset_indices_are_unique() {
        let mut seen = [false; Asset::COUNT];
        for asset in Asset::ALL {
            let idx = asset.index();
            assert!(!seen[idx], "duplicate index {idx} for {asset}");
            seen[idx] = true;
        }
    }

    #[test]
    fn asset_index_covers_range() {
        let mut indices: [usize; Asset::COUNT] = [0; Asset::COUNT];
        for (i, asset) in Asset::ALL.iter().enumerate() {
            indices[i] = asset.index();
        }
        indices.sort_unstable();
        for (expected, got) in indices.iter().enumerate() {
            assert_eq!(expected, *got);
        }
    }

    // Timeframe index uniqueness

    #[test]
    fn timeframe_indices_are_unique() {
        let mut seen = [false; Timeframe::COUNT];
        for tf in Timeframe::ALL {
            let idx = tf.index();
            assert!(!seen[idx], "duplicate index {idx} for {tf}");
            seen[idx] = true;
        }
    }

    // Timeframe duration correctness

    #[test]
    fn timeframe_duration_min5() {
        assert_eq!(Timeframe::Min5.duration_secs(), 300);
    }

    #[test]
    fn timeframe_duration_min15() {
        assert_eq!(Timeframe::Min15.duration_secs(), 900);
    }

    #[test]
    fn timeframe_duration_hour1() {
        assert_eq!(Timeframe::Hour1.duration_secs(), 3_600);
    }

    #[test]
    fn timeframe_duration_hour4() {
        assert_eq!(Timeframe::Hour4.duration_secs(), 14_400);
    }

    // Side involutory opposite

    #[test]
    fn side_opposite_involutory_up() {
        assert_eq!(Side::Up.opposite().opposite(), Side::Up);
    }

    #[test]
    fn side_opposite_involutory_down() {
        assert_eq!(Side::Down.opposite().opposite(), Side::Down);
    }

    #[test]
    fn side_opposite_up_is_down() {
        assert_eq!(Side::Up.opposite(), Side::Down);
    }

    #[test]
    fn side_opposite_down_is_up() {
        assert_eq!(Side::Down.opposite(), Side::Up);
    }

    // Display smoke tests

    #[test]
    fn asset_display() {
        assert_eq!(Asset::Btc.to_string(), "BTC");
        assert_eq!(Asset::Eth.to_string(), "ETH");
        assert_eq!(Asset::Sol.to_string(), "SOL");
        assert_eq!(Asset::Xrp.to_string(), "XRP");
    }

    #[test]
    fn asset_as_lower_str() {
        assert_eq!(Asset::Btc.as_lower_str(), "btc");
        assert_eq!(Asset::Eth.as_lower_str(), "eth");
        assert_eq!(Asset::Sol.as_lower_str(), "sol");
        assert_eq!(Asset::Xrp.as_lower_str(), "xrp");
    }

    #[test]
    fn asset_lower_str_matches_display_lowercase() {
        for asset in Asset::ALL {
            assert_eq!(asset.as_lower_str(), asset.to_string().to_lowercase());
        }
    }

    #[test]
    fn timeframe_display() {
        assert_eq!(Timeframe::Min5.to_string(), "5m");
        assert_eq!(Timeframe::Min15.to_string(), "15m");
        assert_eq!(Timeframe::Hour1.to_string(), "1h");
        assert_eq!(Timeframe::Hour4.to_string(), "4h");
    }

    #[test]
    fn timeframe_as_label() {
        assert_eq!(Timeframe::Min5.as_label(), "5m");
        assert_eq!(Timeframe::Min15.as_label(), "15m");
        assert_eq!(Timeframe::Hour1.as_label(), "1h");
        assert_eq!(Timeframe::Hour4.as_label(), "4h");
    }

    #[test]
    fn timeframe_label_matches_display() {
        for tf in Timeframe::ALL {
            assert_eq!(tf.as_label(), tf.to_string());
        }
    }

    #[test]
    fn exchange_source_display() {
        assert_eq!(ExchangeSource::Binance.to_string(), "Binance");
        assert_eq!(ExchangeSource::Okx.to_string(), "OKX");
    }
}
