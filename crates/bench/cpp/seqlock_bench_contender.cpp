#include "rigtorp_seqlock.h"
#include <cstdint>

extern "C" {

// u64 seqlock
static rigtorp::Seqlock<uint64_t> g_rigtorp_u64;

void rigtorp_seqlock_write_u64(uint64_t val) {
    g_rigtorp_u64.store(val);
}

uint64_t rigtorp_seqlock_read_u64() {
    return g_rigtorp_u64.load();
}

// 64-byte seqlock
struct BenchMsg64 {
    uint8_t data[64];
};

static rigtorp::Seqlock<BenchMsg64> g_rigtorp_64;

void rigtorp_seqlock_write_64(const BenchMsg64* val) {
    g_rigtorp_64.store(*val);
}

void rigtorp_seqlock_read_64(BenchMsg64* out) {
    *out = g_rigtorp_64.load();
}

// 128-byte seqlock
struct BenchMsg128 {
    uint8_t data[128];
};

static rigtorp::Seqlock<BenchMsg128> g_rigtorp_128;

void rigtorp_seqlock_write_128(const BenchMsg128* val) {
    g_rigtorp_128.store(*val);
}

void rigtorp_seqlock_read_128(BenchMsg128* out) {
    *out = g_rigtorp_128.load();
}

}
