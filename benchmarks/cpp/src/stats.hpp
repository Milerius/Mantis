#pragma once

#include <array>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <fstream>
#include <limits>
#include <sstream>
#include <string>

// ---------------------------------------------------------------------------
// CycleHistogram -- fixed-size, allocation-free latency recorder
// ---------------------------------------------------------------------------

/// Number of direct-mapped 1-cycle-resolution buckets.
static constexpr size_t DIRECT_BUCKETS = 4096;

/// Number of logarithmic overflow buckets (covers 4096 .. ~4 billion cycles).
static constexpr size_t LOG_BUCKETS = 20;

/// Map an overflow value (>= 4096) into a log bucket index 0..19.
inline size_t log_bucket(uint64_t cycles) {
    // bucket 0  -> [4096, 8191]
    // bucket 1  -> [8192, 16383]
    // bucket k  -> [4096 << k, (4096 << (k+1)) - 1]
    uint64_t shifted = cycles >> 12; // divide by 4096
    int bits = 63 - __builtin_clzll(shifted); // floor(log2(shifted))
    return static_cast<size_t>(std::min(bits, static_cast<int>(LOG_BUCKETS - 1)));
}

/// Upper bound of the range covered by overflow bucket `i`.
inline uint64_t overflow_upper_bound(size_t i) {
    if (i >= LOG_BUCKETS - 1) {
        return std::numeric_limits<uint64_t>::max();
    }
    // bucket i covers [4096 << i, (4096 << (i+1)) - 1]
    return static_cast<uint64_t>(DIRECT_BUCKETS) << (i + 1);
}

class CycleHistogram {
public:
    CycleHistogram() {
        direct_.fill(0);
        overflow_.fill(0);
    }

    /// Record a single cycle measurement.
    inline void record(uint64_t cycles) {
        count_++;
        sum_ += cycles;
        if (cycles < min_) min_ = cycles;
        if (cycles > max_) max_ = cycles;

        if (cycles < DIRECT_BUCKETS) {
            direct_[static_cast<size_t>(cycles)]++;
        } else {
            overflow_[log_bucket(cycles)]++;
        }
    }

    /// Walk the histogram to find the real percentile value.
    /// `p` is in [0.0, 100.0].
    uint64_t percentile(double p) const {
        if (count_ == 0) return 0;
        uint64_t target = static_cast<uint64_t>(std::ceil((p / 100.0) * static_cast<double>(count_)));
        if (target < 1) target = 1;
        uint64_t accumulated = 0;

        // Walk direct buckets
        for (size_t i = 0; i < DIRECT_BUCKETS; ++i) {
            accumulated += direct_[i];
            if (accumulated >= target) {
                return static_cast<uint64_t>(i);
            }
        }

        // Walk overflow buckets -- return the upper bound of each log range
        for (size_t i = 0; i < LOG_BUCKETS; ++i) {
            accumulated += overflow_[i];
            if (accumulated >= target) {
                return overflow_upper_bound(i);
            }
        }

        return max_;
    }

    double mean() const {
        if (count_ == 0) return 0.0;
        return static_cast<double>(sum_) / static_cast<double>(count_);
    }

    uint64_t max() const { return count_ == 0 ? 0 : max_; }
    uint64_t min() const { return count_ == 0 ? 0 : min_; }
    uint64_t count() const { return count_; }

private:
    std::array<uint64_t, DIRECT_BUCKETS> direct_;
    std::array<uint64_t, LOG_BUCKETS> overflow_;
    uint64_t count_ = 0;
    uint64_t sum_ = 0;
    uint64_t min_ = std::numeric_limits<uint64_t>::max();
    uint64_t max_ = 0;
};

// ---------------------------------------------------------------------------
// System introspection helpers
// ---------------------------------------------------------------------------

inline std::string read_file_trimmed(const char* path) {
    std::ifstream f(path);
    if (!f.is_open()) return "unknown";
    std::string line;
    std::getline(f, line);
    // Trim trailing whitespace
    while (!line.empty() && (line.back() == '\n' || line.back() == '\r' || line.back() == ' '))
        line.pop_back();
    return line;
}

inline std::string read_proc_field(const char* path, const char* field) {
    std::ifstream f(path);
    if (!f.is_open()) return "unknown";
    std::string line;
    while (std::getline(f, line)) {
        if (line.find(field) != std::string::npos) {
            auto pos = line.find(':');
            if (pos != std::string::npos) {
                std::string val = line.substr(pos + 1);
                // Trim leading/trailing whitespace
                size_t start = val.find_first_not_of(" \t");
                if (start == std::string::npos) return "";
                size_t end = val.find_last_not_of(" \t\r\n");
                return val.substr(start, end - start + 1);
            }
            return line;
        }
    }
    return "unknown";
}

inline std::string cpu_model() {
#ifdef __linux__
    return read_proc_field("/proc/cpuinfo", "model name");
#elif defined(__APPLE__)
    char buf[256] = {};
    FILE* fp = popen("sysctl -n machdep.cpu.brand_string 2>/dev/null", "r");
    if (fp) {
        if (fgets(buf, sizeof(buf), fp)) {
            pclose(fp);
            std::string s(buf);
            while (!s.empty() && (s.back() == '\n' || s.back() == '\r'))
                s.pop_back();
            return s;
        }
        pclose(fp);
    }
    return "unknown";
#else
    return "unknown";
#endif
}

inline std::string compiler_version() {
#if defined(__clang__)
    return "clang " + std::to_string(__clang_major__) + "." +
           std::to_string(__clang_minor__) + "." +
           std::to_string(__clang_patchlevel__);
#elif defined(__GNUC__)
    return "gcc " + std::to_string(__GNUC__) + "." +
           std::to_string(__GNUC_MINOR__) + "." +
           std::to_string(__GNUC_PATCHLEVEL__);
#elif defined(_MSC_VER)
    return "msvc " + std::to_string(_MSC_VER);
#else
    return "unknown";
#endif
}

struct SystemInfo {
    std::string governor;
    std::string turbo;
    std::string isolcpus;
    std::string tsc;
    std::string kernel;
};

inline SystemInfo read_system_info() {
    SystemInfo info;
#ifdef __linux__
    info.governor = read_file_trimmed("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor");
    info.turbo = read_file_trimmed("/sys/devices/system/cpu/intel_pstate/no_turbo");

    // Parse isolcpus from /proc/cmdline
    {
        std::ifstream f("/proc/cmdline");
        std::string cmdline;
        std::getline(f, cmdline);
        auto pos = cmdline.find("isolcpus=");
        if (pos != std::string::npos) {
            pos += 9; // skip "isolcpus="
            auto end = cmdline.find(' ', pos);
            info.isolcpus = cmdline.substr(pos, end - pos);
        } else {
            info.isolcpus = "none";
        }
    }

    // Read TSC info from dmesg
    {
        FILE* fp = popen("dmesg 2>/dev/null | grep 'tsc:' | tail -1", "r");
        if (fp) {
            char buf[512] = {};
            if (fgets(buf, sizeof(buf), fp)) {
                info.tsc = buf;
                while (!info.tsc.empty() &&
                       (info.tsc.back() == '\n' || info.tsc.back() == '\r'))
                    info.tsc.pop_back();
            } else {
                info.tsc = "unknown";
            }
            pclose(fp);
        } else {
            info.tsc = "unknown";
        }
    }

    info.kernel = read_file_trimmed("/proc/version");
#else
    info.governor = "n/a";
    info.turbo    = "n/a";
    info.isolcpus = "n/a";
    info.tsc      = "n/a";
    // Use uname -a on non-Linux
    {
        FILE* fp = popen("uname -a 2>/dev/null", "r");
        if (fp) {
            char buf[512] = {};
            if (fgets(buf, sizeof(buf), fp)) {
                info.kernel = buf;
                while (!info.kernel.empty() &&
                       (info.kernel.back() == '\n' || info.kernel.back() == '\r'))
                    info.kernel.pop_back();
            } else {
                info.kernel = "unknown";
            }
            pclose(fp);
        } else {
            info.kernel = "unknown";
        }
    }
#endif
    return info;
}

// ---------------------------------------------------------------------------
// JSON writer -- matches Rust BenchResult schema exactly
// ---------------------------------------------------------------------------

/// Escape a string for JSON output.
inline std::string json_escape(const std::string& s) {
    std::string out;
    out.reserve(s.size() + 8);
    for (char c : s) {
        switch (c) {
            case '"':  out += "\\\""; break;
            case '\\': out += "\\\\"; break;
            case '\n': out += "\\n";  break;
            case '\r': out += "\\r";  break;
            case '\t': out += "\\t";  break;
            default:   out += c;      break;
        }
    }
    return out;
}

/// Write a benchmark result as JSON matching the Rust schema.
inline std::string write_json(
    const CycleHistogram& hist,
    const std::string& implementation,
    size_t producer_core,
    size_t consumer_core,
    size_t capacity,
    size_t message_size_bytes,
    uint64_t warmup_messages
) {
    auto sys = read_system_info();
    auto cpu = cpu_model();
    auto compiler = compiler_version();

    std::ostringstream os;
    os << "{\n";
    os << "  \"implementation\": \"" << json_escape(implementation) << "\",\n";
    os << "  \"language\": \"cpp\",\n";
    os << "  \"version\": \"0.1.0\",\n";
    os << "  \"compiler\": \"" << json_escape(compiler) << "\",\n";
    os << "  \"cpu\": \"" << json_escape(cpu) << "\",\n";
    os << "  \"producer_core\": " << producer_core << ",\n";
    os << "  \"consumer_core\": " << consumer_core << ",\n";
    os << "  \"capacity\": " << capacity << ",\n";
    os << "  \"message_size_bytes\": " << message_size_bytes << ",\n";
    os << "  \"warmup_messages\": " << warmup_messages << ",\n";
    os << "  \"measured_messages\": " << hist.count() << ",\n";
    os << "  \"results\": {\n";
    os << "    \"cycles_per_op_p50\": " << hist.percentile(50.0) << ",\n";
    os << "    \"cycles_per_op_p99\": " << hist.percentile(99.0) << ",\n";
    os << "    \"cycles_per_op_p999\": " << hist.percentile(99.9) << ",\n";
    os << "    \"cycles_per_op_p9999\": " << hist.percentile(99.99) << ",\n";
    os << "    \"cycles_per_op_max\": " << hist.max() << ",\n";
    os << "    \"cycles_per_op_min\": " << hist.min() << ",\n";

    // Format mean with one decimal place
    char mean_buf[64];
    std::snprintf(mean_buf, sizeof(mean_buf), "%.1f", hist.mean());
    os << "    \"cycles_per_op_mean\": " << mean_buf << ",\n";

    os << "    \"total_messages\": " << hist.count() << "\n";
    os << "  },\n";
    os << "  \"system\": {\n";
    os << "    \"governor\": \"" << json_escape(sys.governor) << "\",\n";
    os << "    \"turbo\": \"" << json_escape(sys.turbo) << "\",\n";
    os << "    \"isolcpus\": \"" << json_escape(sys.isolcpus) << "\",\n";
    os << "    \"tsc\": \"" << json_escape(sys.tsc) << "\",\n";
    os << "    \"kernel\": \"" << json_escape(sys.kernel) << "\"\n";
    os << "  }\n";
    os << "}";

    return os.str();
}
