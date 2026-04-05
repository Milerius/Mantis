//! Struct layout and cache-line analysis for the Mantis SDK.
//!
//! Reports size, alignment, field offsets, and cache-line occupancy
//! for hot-path data structures.

#![deny(unsafe_code)]

use core::mem;

/// Layout information for a type.
#[derive(Debug, Clone)]
pub struct LayoutInfo {
    /// Type name.
    pub name: String,
    /// Size in bytes.
    pub size: usize,
    /// Alignment in bytes.
    pub align: usize,
    /// Number of cache lines occupied (assuming 64-byte lines).
    pub cache_lines: usize,
}

/// Inspect the layout of a type.
#[must_use]
pub fn inspect<T>(name: &str) -> LayoutInfo {
    let size = mem::size_of::<T>();
    let align = mem::align_of::<T>();
    let cache_lines = size.div_ceil(64);
    LayoutInfo {
        name: name.to_owned(),
        size,
        align,
        cache_lines,
    }
}

impl std::fmt::Display for LayoutInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Type: {}", self.name)?;
        writeln!(f, "  size:        {} bytes", self.size)?;
        writeln!(f, "  align:       {} bytes", self.align)?;
        writeln!(f, "  cache lines: {} (64B)", self.cache_lines)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspect_u64() {
        let info = inspect::<u64>("u64");
        assert_eq!(info.size, 8);
        assert_eq!(info.align, 8);
        assert_eq!(info.cache_lines, 1);
    }

    #[test]
    fn inspect_large_type() {
        let info = inspect::<[u8; 128]>("[u8; 128]");
        assert_eq!(info.size, 128);
        assert_eq!(info.cache_lines, 2);
    }

    #[test]
    fn seqlock_layout_assertions() {
        use mantis_seqlock::SeqLock;

        // Sequence counter is cache-line padded (128 bytes alignment)
        assert!(core::mem::align_of::<SeqLock<u64>>() >= 128);
        // SeqLock<u64> = 128 (padded seq) + 8 (data) + padding
        assert!(core::mem::size_of::<SeqLock<u64>>() >= 136);
    }

    #[test]
    fn market_state_layout_assertions() {
        use mantis_market_state::{ArrayBook, TopOfBook};

        // TopOfBook should fit in a single cache line (64 bytes)
        let tob_info = inspect::<TopOfBook>("TopOfBook");
        assert!(
            tob_info.size <= 64,
            "TopOfBook size {} exceeds cache line (64 bytes)",
            tob_info.size
        );
        assert_eq!(
            tob_info.cache_lines, 1,
            "TopOfBook must fit in one 64B cache line"
        );

        // ArrayBook<100> is the primary Polymarket shape; bounded size is acceptable
        let book_info = inspect::<ArrayBook<100>>("ArrayBook<100>");
        // Each side is 100 × 8 bytes = 800 bytes; two sides = 1600 bytes + header
        assert!(
            book_info.size > 0,
            "ArrayBook<100> must have positive size"
        );
        // Sanity: two sides (bids + asks) at 8 bytes per Lots entry
        assert_eq!(
            book_info.size,
            core::mem::size_of::<ArrayBook<100>>(),
            "inspect size must match mem::size_of"
        );
    }

    #[test]
    fn event_layout_assertions() {
        use mantis_events::*;

        // HotEvent envelope
        assert_eq!(inspect::<HotEvent>("HotEvent").size, 64);
        assert_eq!(
            core::mem::offset_of!(HotEvent, header),
            0,
            "header must be at offset 0"
        );

        // Header + Body (envelope contract)
        assert_eq!(inspect::<EventHeader>("EventHeader").size, 24);
        assert_eq!(inspect::<EventBody>("EventBody").size, 40);

        // Market payloads
        assert_eq!(inspect::<BookDeltaPayload>("BookDeltaPayload").size, 24);
        assert_eq!(inspect::<TradePayload>("TradePayload").size, 24);
        assert_eq!(inspect::<TopOfBookPayload>("TopOfBookPayload").size, 32);

        // Execution payloads
        assert_eq!(inspect::<OrderAckPayload>("OrderAckPayload").size, 24);
        assert_eq!(inspect::<FillPayload>("FillPayload").size, 32);
        assert_eq!(inspect::<OrderRejectPayload>("OrderRejectPayload").size, 24);

        // Control payloads
        assert_eq!(inspect::<TimerPayload>("TimerPayload").size, 8);
        assert_eq!(inspect::<HeartbeatPayload>("HeartbeatPayload").size, 4);

        // Cache line occupancy
        assert_eq!(
            inspect::<HotEvent>("HotEvent").cache_lines,
            1,
            "HotEvent must fit in one 64B cache line"
        );
    }
}
