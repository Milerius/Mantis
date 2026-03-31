// FFI wrapper around rigtorp::SPSCQueue for Rust benchmarks.
//
// Exposes extern "C" functions for create/destroy/try_push/try_pop
// for u64, Message48, and Message64 element types.

#include "rigtorp/SPSCQueue.h"
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

void *rigtorp_u64_create(size_t capacity) {
    return new rigtorp::SPSCQueue<uint64_t>(capacity);
}

void rigtorp_u64_destroy(void *q) {
    delete static_cast<rigtorp::SPSCQueue<uint64_t> *>(q);
}

bool rigtorp_u64_try_push(void *q, uint64_t value) {
    return static_cast<rigtorp::SPSCQueue<uint64_t> *>(q)
        ->try_push(value);
}

bool rigtorp_u64_try_pop(void *q, uint64_t *out) {
    auto *queue = static_cast<rigtorp::SPSCQueue<uint64_t> *>(q);
    auto *front = queue->front();
    if (front == nullptr) {
        return false;
    }
    *out = *front;
    queue->pop();
    return true;
}

// ---------------------------------------------------------------------------
// Message48
// ---------------------------------------------------------------------------

void *rigtorp_msg48_create(size_t capacity) {
    return new rigtorp::SPSCQueue<Message48>(capacity);
}

void rigtorp_msg48_destroy(void *q) {
    delete static_cast<rigtorp::SPSCQueue<Message48> *>(q);
}

bool rigtorp_msg48_try_push(void *q, const Message48 *value) {
    return static_cast<rigtorp::SPSCQueue<Message48> *>(q)
        ->try_push(*value);
}

bool rigtorp_msg48_try_pop(void *q, Message48 *out) {
    auto *queue = static_cast<rigtorp::SPSCQueue<Message48> *>(q);
    auto *front = queue->front();
    if (front == nullptr) {
        return false;
    }
    *out = *front;
    queue->pop();
    return true;
}

// ---------------------------------------------------------------------------
// Message64
// ---------------------------------------------------------------------------

void *rigtorp_msg64_create(size_t capacity) {
    return new rigtorp::SPSCQueue<Message64>(capacity);
}

void rigtorp_msg64_destroy(void *q) {
    delete static_cast<rigtorp::SPSCQueue<Message64> *>(q);
}

bool rigtorp_msg64_try_push(void *q, const Message64 *value) {
    return static_cast<rigtorp::SPSCQueue<Message64> *>(q)
        ->try_push(*value);
}

bool rigtorp_msg64_try_pop(void *q, Message64 *out) {
    auto *queue = static_cast<rigtorp::SPSCQueue<Message64> *>(q);
    auto *front = queue->front();
    if (front == nullptr) {
        return false;
    }
    *out = *front;
    queue->pop();
    return true;
}

} // extern "C"
