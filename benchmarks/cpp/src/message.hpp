#pragma once

#include <cstdint>

/// 48-byte benchmark message with rdtsc timestamp field.
/// Layout matches Rust Message48 exactly.
struct alignas(16) Message48 {
    uint64_t timestamp;   // rdtsc stamped by producer right before push
    uint32_t symbol_id;
    uint16_t side;
    uint16_t flags;
    int64_t  price;
    int64_t  quantity;
    int64_t  order_id;
    uint64_t sequence;
};

static_assert(sizeof(Message48) == 48, "Message48 must be exactly 48 bytes");

/// Create a deterministic message for index `i`.
inline Message48 make_msg(uint64_t i) {
    Message48 msg{};
    msg.timestamp = 0;
    msg.symbol_id = static_cast<uint32_t>(i);
    msg.side      = static_cast<uint16_t>(i & 1);
    msg.flags     = static_cast<uint16_t>(i & 0x3);
    msg.price     = static_cast<int64_t>(i) * 10;
    msg.quantity  = static_cast<int64_t>(i) * 100;
    msg.order_id  = static_cast<int64_t>(i) * 1000;
    msg.sequence  = i;
    return msg;
}
