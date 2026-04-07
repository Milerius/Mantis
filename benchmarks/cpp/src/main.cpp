#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <string>
#include <vector>

#include "harness.hpp"
#include "message.hpp"
#include "stats.hpp"
#include "queues/rigtorp.hpp"
#include "queues/drogalis.hpp"

// ---------------------------------------------------------------------------
// CLI argument parsing
// ---------------------------------------------------------------------------

struct Args {
    std::string queue       = "all";
    std::string mode        = "histogram";
    unsigned producer_core  = 0;
    unsigned consumer_core  = 1;
    uint64_t messages       = 1'000'000;
    uint64_t warmup         = 10'000;
    unsigned runs           = 5;
    std::string output_dir  = "results";
};

static void usage(const char* prog) {
    fprintf(stderr,
        "Usage: %s [OPTIONS]\n"
        "  --queue all|rigtorp|drogalis\n"
        "  --mode histogram|raw  (default: histogram)\n"
        "  --producer-core N\n"
        "  --consumer-core N\n"
        "  --messages N\n"
        "  --warmup N\n"
        "  --runs N\n"
        "  --output-dir DIR\n",
        prog);
}

static Args parse_args(int argc, char* argv[]) {
    Args args;
    for (int i = 1; i < argc; ++i) {
        auto match = [&](const char* flag) { return strcmp(argv[i], flag) == 0; };
        auto next  = [&]() -> const char* {
            if (i + 1 >= argc) {
                fprintf(stderr, "missing value for %s\n", argv[i]);
                exit(1);
            }
            return argv[++i];
        };

        if (match("--queue"))          args.queue = next();
        else if (match("--mode"))     args.mode = next();
        else if (match("--producer-core")) args.producer_core = static_cast<unsigned>(atoi(next()));
        else if (match("--consumer-core")) args.consumer_core = static_cast<unsigned>(atoi(next()));
        else if (match("--messages"))  args.messages = static_cast<uint64_t>(atoll(next()));
        else if (match("--warmup"))    args.warmup   = static_cast<uint64_t>(atoll(next()));
        else if (match("--runs"))      args.runs     = static_cast<unsigned>(atoi(next()));
        else if (match("--output-dir")) args.output_dir = next();
        else if (match("--help") || match("-h")) { usage(argv[0]); exit(0); }
        else { fprintf(stderr, "unknown flag: %s\n", argv[i]); usage(argv[0]); exit(1); }
    }
    return args;
}

// ---------------------------------------------------------------------------
// Capacity and message size constants (match Rust)
// ---------------------------------------------------------------------------

static constexpr size_t CAPACITY = 1024;
static constexpr size_t MESSAGE_SIZE = 48;

// ---------------------------------------------------------------------------
// Raw queue runner (sum += delta, no Vec, no histogram)
// ---------------------------------------------------------------------------

/// Run a single raw-protocol measurement for a queue adapter.
/// Returns total cycle sum across all ops.
template <typename Adapter>
uint64_t run_raw_queue_once(Adapter& adapter, const Args& args) {
    auto push_fn = adapter.make_push();
    auto pop_fn  = adapter.make_pop();

    std::atomic<bool> consumer_ready{false};
    std::atomic<bool> producer_ready{false};
    std::atomic<uint64_t> total_latency{0};

    std::thread consumer_thread([&]() {
        pin_thread(args.consumer_core);

        Message48 msg{};
        uint64_t sum = 0;
        uint64_t count = 0;

        consumer_ready.store(true, std::memory_order_release);

        while (!producer_ready.load(std::memory_order_acquire)) {
            // spin
        }

        while (count < args.messages) {
            if (pop_fn(msg)) {
                uint64_t now = rdtsc_serialized();
                sum += now - msg.timestamp;
                count++;
            }
        }
        total_latency.store(sum, std::memory_order_release);
    });

    std::thread producer_thread([&]() {
        pin_thread(args.producer_core);

        producer_ready.store(true, std::memory_order_release);

        while (!consumer_ready.load(std::memory_order_acquire)) {
            // spin
        }

        for (uint64_t i = 0; i < args.messages; ++i) {
            Message48 msg = make_msg(i);
            msg.timestamp = rdtsc_serialized();
            while (!push_fn(msg)) {
                // spin
            }
        }
    });

    producer_thread.join();
    consumer_thread.join();

    return total_latency.load(std::memory_order_acquire);
}

/// Run N iterations of the raw protocol for a given queue, print best.
template <typename Adapter>
void run_raw_queue(const Args& args) {
    fprintf(stderr, "[%s] (raw mode)\n", Adapter::name);

    // Warmup
    {
        Adapter warmup_adapter(CAPACITY);
        run_raw_queue_once(warmup_adapter, args);
    }

    uint64_t best = UINT64_MAX;
    for (unsigned run = 1; run <= args.runs; ++run) {
        Adapter adapter(CAPACITY);
        uint64_t total_cycles = run_raw_queue_once(adapter, args);
        double cycles_per_op = static_cast<double>(total_cycles) / static_cast<double>(args.messages);
        if (total_cycles < best) {
            best = total_cycles;
        }
        fprintf(stderr, "  run %u/%u: %.1f cycles/op\n", run, args.runs, cycles_per_op);
    }
    double best_per_op = static_cast<double>(best) / static_cast<double>(args.messages);
    fprintf(stderr, "  BEST: %.1f cycles/op\n", best_per_op);
}

// ---------------------------------------------------------------------------
// Histogram queue runners
// ---------------------------------------------------------------------------

template <typename Adapter>
void run_queue_impl(Adapter& adapter, const Args& args, unsigned run) {
    auto push_fn = adapter.make_push();
    auto pop_fn  = adapter.make_pop();

    CycleHistogram hist = run_bench(
        push_fn, pop_fn,
        args.producer_core, args.consumer_core,
        args.warmup, args.messages
    );

    // Print per-run summary to stderr
    fprintf(stderr, "  run %u/%u: p50=%lu p99=%lu p999=%lu max=%lu mean=%.1f cycles/op\n",
            run, args.runs,
            static_cast<unsigned long>(hist.percentile(50.0)),
            static_cast<unsigned long>(hist.percentile(99.0)),
            static_cast<unsigned long>(hist.percentile(99.9)),
            static_cast<unsigned long>(hist.max()),
            hist.mean());

    // Write JSON
    std::string json = write_json(
        hist, Adapter::name,
        args.producer_core, args.consumer_core,
        CAPACITY, MESSAGE_SIZE, args.warmup
    );

    std::string filename = args.output_dir + "/cpp_" +
                           std::string(Adapter::name) + "_run_" +
                           std::to_string(run) + ".json";

    std::ofstream out(filename);
    if (!out.is_open()) {
        fprintf(stderr, "failed to write %s\n", filename.c_str());
        exit(1);
    }
    out << json;
}

void run_queue(const std::string& name, const Args& args, unsigned run) {
    if (name == "rigtorp") {
        RigtorpAdapter adapter(CAPACITY);
        run_queue_impl(adapter, args, run);
    } else if (name == "drogalis") {
        DrogalisAdapter adapter(CAPACITY);
        run_queue_impl(adapter, args, run);
    } else {
        fprintf(stderr, "unknown queue: %s\n", name.c_str());
        exit(1);
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

int main(int argc, char* argv[]) {
    Args args = parse_args(argc, argv);

    // Determine which queues to run
    std::vector<std::string> queues;
    if (args.queue == "all") {
        queues = {"rigtorp", "drogalis"};
    } else {
        queues = {args.queue};
    }

    if (args.mode == "raw") {
        // Raw mode: sum += delta, no histogram, no JSON output
        for (const auto& name : queues) {
            if (name == "rigtorp") {
                run_raw_queue<RigtorpAdapter>(args);
            } else if (name == "drogalis") {
                run_raw_queue<DrogalisAdapter>(args);
            } else {
                fprintf(stderr, "unknown queue: %s\n", name.c_str());
                return 1;
            }
            fprintf(stderr, "\n");
        }
    } else {
        // Histogram mode (default): full protocol with JSON output
        std::filesystem::create_directories(args.output_dir);

        for (const auto& name : queues) {
            fprintf(stderr, "[%s]\n", name.c_str());
            for (unsigned run = 1; run <= args.runs; ++run) {
                run_queue(name, args, run);
            }
        }
    }

    return 0;
}
