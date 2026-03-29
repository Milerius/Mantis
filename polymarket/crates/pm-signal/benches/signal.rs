//! Criterion benchmarks for [`pm_signal`] evaluation paths.
//!
//! Target: < 50 ns per `evaluate()` call.

#![expect(clippy::expect_used, reason = "bench setup uses expect for conciseness")]
#![expect(missing_docs, reason = "criterion macros generate undocumented items")]

use criterion::{criterion_group, criterion_main, Criterion};
use pm_signal::{
    Coefficients, FairValueEstimator, LogisticModel, LookupTable, SignalEngine, MAG_BUCKETS,
    TIME_BUCKETS,
};
use pm_types::{Asset, ContractPrice, ExchangeSource, Price, Tick, Timeframe, Window, WindowId};

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn make_tick() -> Tick {
    Tick {
        asset: Asset::Btc,
        price: Price::new(42_500.0).expect("valid price"),
        timestamp_ms: 1_800_000,
        source: ExchangeSource::Binance,
    }
}

fn make_window() -> Window {
    Window {
        id: WindowId::new(1),
        asset: Asset::Btc,
        timeframe: Timeframe::Hour1,
        open_time_ms: 0,
        close_time_ms: 3_600_000,
        open_price: Price::new(42_000.0).expect("valid price"),
    }
}

fn make_lookup_table() -> LookupTable {
    let mut table = LookupTable::new(5);
    // Populate every cell so we never fall back to 0.5.
    for (ai, asset) in Asset::ALL.iter().enumerate() {
        for (ti, timeframe) in Timeframe::ALL.iter().enumerate() {
            for mb in 0..MAG_BUCKETS {
                for tb in 0..TIME_BUCKETS {
                    // Index arithmetic stays small; no precision concern.
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "index values are small (< 100), no precision loss in practice"
                    )]
                    let prob = (0.45 + (ai * 4 + ti * 2 + mb + tb) as f64 * 0.001).min(0.95);
                    table.set(*asset, *timeframe, mb, tb, prob, 100);
                }
            }
        }
    }
    table
}

fn make_logistic_model() -> LogisticModel {
    let mut model = LogisticModel::new();
    model.set_coefficients(
        Asset::Btc,
        Timeframe::Hour1,
        Coefficients { beta_0: 0.5, beta_1: 30.0, beta_2: -0.2, beta_3: 5.0 },
    );
    model
}

// ─── Benchmarks ──────────────────────────────────────────────────────────────

fn bench_lookup_estimate(c: &mut Criterion) {
    let table = make_lookup_table();
    c.bench_function("lookup_estimate", |b| {
        b.iter(|| {
            criterion::black_box(table.estimate(
                criterion::black_box(0.003),
                criterion::black_box(300),
                criterion::black_box(Asset::Btc),
                criterion::black_box(Timeframe::Hour1),
            ));
        });
    });
}

fn bench_logistic_estimate(c: &mut Criterion) {
    let model = make_logistic_model();
    c.bench_function("logistic_estimate", |b| {
        b.iter(|| {
            criterion::black_box(model.estimate(
                criterion::black_box(0.003),
                criterion::black_box(300),
                criterion::black_box(Asset::Btc),
                criterion::black_box(Timeframe::Hour1),
            ));
        });
    });
}

fn bench_signal_engine_evaluate_lookup(c: &mut Criterion) {
    let table = make_lookup_table();
    let engine = SignalEngine::new(table, 0.02);
    let tick = make_tick();
    let window = make_window();
    let market_price = ContractPrice::new(0.48).expect("valid");

    c.bench_function("signal_engine_evaluate_lookup", |b| {
        b.iter(|| {
            criterion::black_box(engine.evaluate(
                criterion::black_box(&tick),
                criterion::black_box(&window),
                criterion::black_box(market_price),
            ));
        });
    });
}

fn bench_signal_engine_evaluate_logistic(c: &mut Criterion) {
    let model = make_logistic_model();
    let engine = SignalEngine::new(model, 0.02);
    let tick = make_tick();
    let window = make_window();
    let market_price = ContractPrice::new(0.48).expect("valid");

    c.bench_function("signal_engine_evaluate_logistic", |b| {
        b.iter(|| {
            criterion::black_box(engine.evaluate(
                criterion::black_box(&tick),
                criterion::black_box(&window),
                criterion::black_box(market_price),
            ));
        });
    });
}

criterion_group!(
    benches,
    bench_lookup_estimate,
    bench_logistic_estimate,
    bench_signal_engine_evaluate_lookup,
    bench_signal_engine_evaluate_logistic,
);
criterion_main!(benches);
