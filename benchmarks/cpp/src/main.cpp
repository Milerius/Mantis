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
// Queue runners
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

    // Create output directory
    std::filesystem::create_directories(args.output_dir);

    // Determine which queues to run
    std::vector<std::string> queues;
    if (args.queue == "all") {
        queues = {"rigtorp", "drogalis"};
    } else {
        queues = {args.queue};
    }

    for (const auto& name : queues) {
        fprintf(stderr, "[%s]\n", name.c_str());
        for (unsigned run = 1; run <= args.runs; ++run) {
            run_queue(name, args, run);
        }
    }

    return 0;
}
