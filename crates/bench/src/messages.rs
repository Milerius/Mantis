//! Realistic financial message types for benchmarks.
//!
//! These hit the exact SIMD kernel sizes (48, 64 bytes) to exercise
//! the copy-optimized ring's hot paths.

use core::mem::size_of;

/// 48-byte financial order message aligned to 16 bytes.
#[repr(C, align(16))]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Message48 {
    /// Exchange timestamp in nanoseconds.
    pub timestamp: u64,
    /// Instrument symbol identifier.
    pub symbol_id: u32,
    /// Order side (0 = bid, 1 = ask).
    pub side: u16,
    /// Message flags bitmask.
    pub flags: u16,
    /// Limit price in ticks.
    pub price: i64,
    /// Order quantity.
    pub quantity: i64,
    /// Exchange order identifier.
    pub order_id: i64,
    /// Sequence number.
    pub sequence: u64,
}

const _: () = assert!(size_of::<Message48>() == 48);

/// 64-byte financial order message aligned to 16 bytes.
#[repr(C, align(16))]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Message64 {
    /// Exchange timestamp in nanoseconds.
    pub timestamp: u64,
    /// Instrument symbol identifier.
    pub symbol_id: u32,
    /// Order side (0 = bid, 1 = ask).
    pub side: u16,
    /// Message flags bitmask.
    pub flags: u16,
    /// Limit price in ticks.
    pub price: i64,
    /// Order quantity.
    pub quantity: i64,
    /// Exchange order identifier.
    pub order_id: i64,
    /// Sequence number.
    pub sequence: u64,
    /// Venue/exchange identifier.
    pub venue_id: u32,
    /// Explicit padding to reach 64 bytes.
    _pad: u32,
    /// Client-assigned order identifier.
    pub client_order_id: u64,
}

const _: () = assert!(size_of::<Message64>() == 64);

/// Deterministic test message for reproducible benchmarks.
#[must_use]
#[expect(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
pub fn make_msg48(i: u64) -> Message48 {
    Message48 {
        timestamp: i,
        symbol_id: i as u32,
        side: (i & 1) as u16,
        flags: (i & 0x3) as u16,
        price: i as i64 * 10,
        quantity: i as i64 * 100,
        order_id: i as i64 * 1000,
        sequence: i,
    }
}

/// Deterministic test message for reproducible benchmarks.
#[must_use]
#[expect(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
pub fn make_msg64(i: u64) -> Message64 {
    Message64 {
        timestamp: i,
        symbol_id: i as u32,
        side: (i & 1) as u16,
        flags: (i & 0x3) as u16,
        price: i as i64 * 10,
        quantity: i as i64 * 100,
        order_id: i as i64 * 1000,
        sequence: i,
        venue_id: (i & 0xFF) as u32,
        _pad: 0,
        client_order_id: i * 10_000,
    }
}
