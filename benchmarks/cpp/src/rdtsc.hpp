#pragma once

#include <cstdint>

#if defined(__x86_64__) || defined(_M_X64)
#include <immintrin.h>

/// Read TSC with serialization (lfence before rdtsc).
inline uint64_t rdtsc_serialized() {
    _mm_lfence();
    return __rdtsc();
}

#elif defined(__aarch64__)
#include <chrono>

/// Fallback for non-x86_64: use monotonic clock in nanoseconds.
inline uint64_t rdtsc_serialized() {
    auto now = std::chrono::steady_clock::now();
    return static_cast<uint64_t>(
        std::chrono::duration_cast<std::chrono::nanoseconds>(
            now.time_since_epoch()
        ).count()
    );
}

#else
#include <chrono>

inline uint64_t rdtsc_serialized() {
    auto now = std::chrono::steady_clock::now();
    return static_cast<uint64_t>(
        std::chrono::duration_cast<std::chrono::nanoseconds>(
            now.time_since_epoch()
        ).count()
    );
}

#endif
