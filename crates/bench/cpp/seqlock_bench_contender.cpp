#include "rigtorp_seqlock.h"
#include <cstdint>

extern "C" {

struct BenchMsg64 {
    uint8_t data[64];
};

static rigtorp::Seqlock<BenchMsg64> g_rigtorp_lock;

void rigtorp_seqlock_write_64(const BenchMsg64* val) {
    g_rigtorp_lock.store(*val);
}

void rigtorp_seqlock_read_64(BenchMsg64* out) {
    *out = g_rigtorp_lock.load();
}

}
