//! Two-thread SPSC queue latency benchmark.
//!
//! Uses the HFT University protocol: two threads on isolated cores,
//! rdtsc timestamping, sum += delta per message.
//!
//! Tests all mantis-queue variants (SpscRing, SpscRingFast, SpscRingCopy)
//! and rtrb for comparison.

mod raw_bench;

use clap::Parser;

#[derive(Parser)]
#[command(name = "mantis-spsc-bench")]
#[command(about = "Two-thread SPSC queue latency benchmark (HFT University protocol)")]
struct Args {
    /// Logical core id for the producer thread
    #[arg(long)]
    producer_core: usize,

    /// Logical core id for the consumer thread
    #[arg(long)]
    consumer_core: usize,

    /// Number of messages per run
    #[arg(long, default_value_t = 1_000_000)]
    messages: u64,

    /// Number of runs per queue
    #[arg(long, default_value_t = 10)]
    runs: usize,
}

fn main() {
    let args = Args::parse();

    raw_bench::run_raw_bench(
        args.producer_core,
        args.consumer_core,
        args.messages,
        args.runs,
    );
}
