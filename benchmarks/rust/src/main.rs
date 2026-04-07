mod harness;
mod message;
mod queues;
mod rdtsc;
mod stats;

use std::fs;

use clap::Parser;

use crate::queues::mantis_copy::MantisCopyQueue;
use crate::queues::mantis_inline::MantisInlineQueue;
use crate::queues::rtrb_queue::RtrbQueue;
use crate::stats::BenchResult;

#[derive(Parser)]
#[command(name = "mantis-spsc-bench")]
#[command(about = "Two-thread SPSC queue latency benchmark")]
struct Args {
    /// Queue implementation to benchmark: "mantis-inline", "mantis-copy", "rtrb", or "all"
    #[arg(long, default_value = "all")]
    queue: String,

    /// Logical core id for the producer thread
    #[arg(long)]
    producer_core: usize,

    /// Logical core id for the consumer thread
    #[arg(long)]
    consumer_core: usize,

    /// Number of measured messages per run
    #[arg(long, default_value_t = 1_000_000)]
    messages: u64,

    /// Number of warmup messages before measurement begins
    #[arg(long, default_value_t = 10_000)]
    warmup: u64,

    /// Number of runs per queue
    #[arg(long, default_value_t = 5)]
    runs: usize,

    /// Directory for JSON result files
    #[arg(long, default_value = "results")]
    output_dir: std::path::PathBuf,
}

const CAPACITY: usize = 1024;
const MESSAGE_SIZE: usize = 48;

fn run_queue(
    name: &str,
    args: &Args,
    run: usize,
) {
    let hist = match name {
        "mantis-inline" => harness::run_bench(
            MantisInlineQueue::new(),
            args.producer_core,
            args.consumer_core,
            args.warmup,
            args.messages,
        ),
        "mantis-copy" => harness::run_bench(
            MantisCopyQueue::new(),
            args.producer_core,
            args.consumer_core,
            args.warmup,
            args.messages,
        ),
        "rtrb" => harness::run_bench(
            RtrbQueue::new(),
            args.producer_core,
            args.consumer_core,
            args.warmup,
            args.messages,
        ),
        _ => {
            eprintln!("unknown queue: {name}");
            std::process::exit(1);
        }
    };

    let result = BenchResult::from_histogram(
        &hist,
        name,
        args.producer_core,
        args.consumer_core,
        CAPACITY,
        MESSAGE_SIZE,
        args.warmup,
    );

    eprintln!(
        "  run {run}/{}: p50={} p99={} p999={} max={} mean={:.1} cycles/op",
        args.runs,
        result.results.cycles_per_op_p50,
        result.results.cycles_per_op_p99,
        result.results.cycles_per_op_p999,
        result.results.cycles_per_op_max,
        result.results.cycles_per_op_mean,
    );

    let json = result.to_json();
    let filename = args.output_dir.join(format!("rust_{name}_run_{run}.json"));
    fs::write(&filename, &json).unwrap_or_else(|e| {
        eprintln!("failed to write {}: {e}", filename.display());
        std::process::exit(1);
    });
}

fn main() {
    let args = Args::parse();

    fs::create_dir_all(&args.output_dir).unwrap_or_else(|e| {
        eprintln!("failed to create output dir {}: {e}", args.output_dir.display());
        std::process::exit(1);
    });

    let queues: Vec<&str> = if args.queue == "all" {
        vec!["mantis-inline", "mantis-copy", "rtrb"]
    } else {
        vec![args.queue.as_str()]
    };

    for name in &queues {
        eprintln!("[{name}]");
        for run in 1..=args.runs {
            run_queue(name, &args, run);
        }
    }
}
