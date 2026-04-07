#pragma once

#include <atomic>
#include <cerrno>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <thread>

#include "message.hpp"
#include "rdtsc.hpp"
#include "stats.hpp"

// ---------------------------------------------------------------------------
// Core pinning
// ---------------------------------------------------------------------------

#ifdef __linux__
#include <pthread.h>
#include <sched.h>

inline void pin_thread(unsigned core_id) {
    cpu_set_t cpuset;
    CPU_ZERO(&cpuset);
    CPU_SET(core_id, &cpuset);
    int rc = sched_setaffinity(0, sizeof(cpu_set_t), &cpuset);
    if (rc != 0) {
        fprintf(stderr, "ERROR: sched_setaffinity failed for core %u (errno=%d)\n", core_id, errno);
        exit(1);
    }
}

#elif defined(__APPLE__)
#include <mach/mach.h>
#include <mach/thread_policy.h>
#include <pthread.h>

inline void pin_thread(unsigned core_id) {
    // macOS does not support hard pinning; use affinity hint.
    thread_affinity_policy_data_t policy = {static_cast<integer_t>(core_id + 1)};
    thread_policy_set(
        pthread_mach_thread_np(pthread_self()),
        THREAD_AFFINITY_POLICY,
        reinterpret_cast<thread_policy_t>(&policy),
        1
    );
}

#else
inline void pin_thread(unsigned /*core_id*/) {
    // No pinning support on this platform.
}
#endif

// ---------------------------------------------------------------------------
// Two-thread SPSC benchmark harness
// ---------------------------------------------------------------------------

/// Run a two-thread SPSC latency benchmark.
///
/// push_fn:  (const Message48&) -> bool   (try_push semantics)
/// pop_fn:   (Message48&)       -> bool   (try_pop semantics)
///
/// The producer runs on the calling thread (pinned to producer_core),
/// the consumer runs on a spawned thread (pinned to consumer_core).
/// Returns the consumer-side latency histogram.
template <typename PushFn, typename PopFn>
CycleHistogram run_bench(
    PushFn push_fn,
    PopFn pop_fn,
    unsigned producer_core,
    unsigned consumer_core,
    uint64_t warmup,
    uint64_t messages
) {
    const uint64_t total = warmup + messages;

    std::atomic<bool> consumer_ready{false};
    std::atomic<bool> producer_ready{false};

    CycleHistogram result_hist;
    std::atomic<bool> consumer_done{false};

    // Spawn consumer thread
    std::thread consumer_thread([&]() {
        pin_thread(consumer_core);

        CycleHistogram histogram;
        Message48 msg{};
        uint64_t received = 0;

        // Signal consumer is ready
        consumer_ready.store(true, std::memory_order_release);

        // Wait for producer to be ready
        while (!producer_ready.load(std::memory_order_acquire)) {
            // spin
        }

        // Consumer loop
        while (received < total) {
            if (pop_fn(msg)) {
                if (received >= warmup) {
                    uint64_t now = rdtsc_serialized();
                    uint64_t delta = now - msg.timestamp;
                    histogram.record(delta);
                }
                received++;
            }
        }

        result_hist = histogram;
        consumer_done.store(true, std::memory_order_release);
    });

    // Pin producer to its core
    pin_thread(producer_core);

    // Wait for consumer to be ready
    while (!consumer_ready.load(std::memory_order_acquire)) {
        // spin
    }

    // Signal producer is ready
    producer_ready.store(true, std::memory_order_release);

    // Producer loop
    for (uint64_t i = 0; i < total; ++i) {
        Message48 msg = make_msg(i);
        msg.timestamp = rdtsc_serialized();

        while (!push_fn(msg)) {
            // spin
        }
    }

    consumer_thread.join();

    return result_hist;
}
