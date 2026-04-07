#pragma once
/// Two-thread SPSC benchmark harness with core pinning and rdtsc timestamping.
///
/// Protocol matches HFT University's ring buffer challenge:
/// - Both producer and consumer are spawned threads (pinned to isolated cores)
/// - Consumer records raw cycle deltas into a pre-allocated vector
/// - Histogram built after the run (no hot-loop cache pollution)

#include <atomic>
#include <cerrno>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <thread>
#include <vector>

#include "message.hpp"
#include "rdtsc.hpp"
#include "stats.hpp"

// ---------------------------------------------------------------------------
// Core pinning via sched_setaffinity (works on isolcpus cores)
// ---------------------------------------------------------------------------

#ifdef __linux__
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
/// Both producer and consumer run on spawned threads. Consumer records
/// raw cycle deltas into a pre-allocated vector, then builds the histogram
/// after the measurement loop (no cache pollution in the hot path).
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

    // Spawn consumer thread
    std::thread consumer_thread([&]() {
        pin_thread(consumer_core);

        Message48 msg{};
        uint64_t received = 0;

        // Pre-allocate storage for raw deltas
        std::vector<uint64_t> deltas;
        deltas.reserve(messages);

        // Signal consumer is ready
        consumer_ready.store(true, std::memory_order_release);

        // Wait for producer to be ready
        while (!producer_ready.load(std::memory_order_acquire)) {
            // spin
        }

        // Consumer loop — only record raw delta, no histogram work
        while (received < total) {
            if (pop_fn(msg)) {
                if (received >= warmup) {
                    uint64_t now = rdtsc_serialized();
                    deltas.push_back(now - msg.timestamp);
                }
                received++;
            }
        }

        // Build histogram from collected deltas (cold path)
        CycleHistogram histogram;
        for (uint64_t d : deltas) {
            histogram.record(d);
        }
        result_hist = histogram;
    });

    // Spawn producer thread
    std::thread producer_thread([&]() {
        pin_thread(producer_core);

        // Signal producer is ready
        producer_ready.store(true, std::memory_order_release);

        // Wait for consumer to be ready
        while (!consumer_ready.load(std::memory_order_acquire)) {
            // spin
        }

        // Producer loop
        for (uint64_t i = 0; i < total; ++i) {
            Message48 msg = make_msg(i);
            msg.timestamp = rdtsc_serialized();

            while (!push_fn(msg)) {
                // spin
            }
        }
    });

    producer_thread.join();
    consumer_thread.join();

    return result_hist;
}
