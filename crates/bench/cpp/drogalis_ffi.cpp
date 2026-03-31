// FFI wrapper around dro::SPSCQueue for Rust benchmarks.
//
// Exposes extern "C" functions for create/destroy/try_push/try_pop
// for u64, Message48, and Message64 element types.
//
// Requires C++20 (dro::SPSCQueue uses concepts).

#include "dro/spsc-queue.hpp"
#include <cstddef>
#include <cstdint>
#include <cstring>

// Mirror Rust repr(C, align(16)) Message48 (48 bytes)
struct alignas(16) Message48 {
    uint64_t timestamp;
    uint32_t symbol_id;
    uint16_t side;
    uint16_t flags;
    int64_t price;
    int64_t quantity;
    int64_t order_id;
    uint64_t sequence;
};
static_assert(sizeof(Message48) == 48, "Message48 size mismatch");

// Mirror Rust repr(C, align(16)) Message64 (64 bytes)
struct alignas(16) Message64 {
    uint64_t timestamp;
    uint32_t symbol_id;
    uint16_t side;
    uint16_t flags;
    int64_t price;
    int64_t quantity;
    int64_t order_id;
    uint64_t sequence;
    uint32_t venue_id;
    uint32_t _pad;
    uint64_t client_order_id;
};
static_assert(sizeof(Message64) == 64, "Message64 size mismatch");

// ---------------------------------------------------------------------------
// u64
// ---------------------------------------------------------------------------

extern "C" {

void *drogalis_u64_create(size_t capacity) {
    return new dro::SPSCQueue<uint64_t>(capacity);
}

void drogalis_u64_destroy(void *q) {
    delete static_cast<dro::SPSCQueue<uint64_t> *>(q);
}

bool drogalis_u64_try_push(void *q, uint64_t value) {
    return static_cast<dro::SPSCQueue<uint64_t> *>(q)->try_push(value);
}

bool drogalis_u64_try_pop(void *q, uint64_t *out) {
    return static_cast<dro::SPSCQueue<uint64_t> *>(q)->try_pop(*out);
}

// ---------------------------------------------------------------------------
// Message48
// ---------------------------------------------------------------------------

void *drogalis_msg48_create(size_t capacity) {
    return new dro::SPSCQueue<Message48>(capacity);
}

void drogalis_msg48_destroy(void *q) {
    delete static_cast<dro::SPSCQueue<Message48> *>(q);
}

bool drogalis_msg48_try_push(void *q, const Message48 *value) {
    return static_cast<dro::SPSCQueue<Message48> *>(q)->try_push(*value);
}

bool drogalis_msg48_try_pop(void *q, Message48 *out) {
    return static_cast<dro::SPSCQueue<Message48> *>(q)->try_pop(*out);
}

// ---------------------------------------------------------------------------
// Message64
// ---------------------------------------------------------------------------

void *drogalis_msg64_create(size_t capacity) {
    return new dro::SPSCQueue<Message64>(capacity);
}

void drogalis_msg64_destroy(void *q) {
    delete static_cast<dro::SPSCQueue<Message64> *>(q);
}

bool drogalis_msg64_try_push(void *q, const Message64 *value) {
    return static_cast<dro::SPSCQueue<Message64> *>(q)->try_push(*value);
}

bool drogalis_msg64_try_pop(void *q, Message64 *out) {
    return static_cast<dro::SPSCQueue<Message64> *>(q)->try_pop(*out);
}

} // extern "C"
