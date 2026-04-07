//! 48-byte financial message for SPSC benchmarks.

/// 48-byte benchmark message with rdtsc timestamp field.
#[repr(C, align(16))]
#[derive(Clone, Copy, Default, Debug)]
pub struct Message48 {
    pub timestamp: u64,    // rdtsc stamped by producer right before push
    pub symbol_id: u32,
    pub side: u16,
    pub flags: u16,
    pub price: i64,
    pub quantity: i64,
    pub order_id: i64,
    pub sequence: u64,
}

const _: () = assert!(core::mem::size_of::<Message48>() == 48);

/// Create a deterministic message for index `i`.
#[inline]
#[expect(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
pub fn make_msg(i: u64) -> Message48 {
    Message48 {
        timestamp: 0,
        symbol_id: i as u32,
        side: (i & 1) as u16,
        flags: (i & 0x3) as u16,
        price: i as i64 * 10,
        quantity: i as i64 * 100,
        order_id: i as i64 * 1000,
        sequence: i,
    }
}
