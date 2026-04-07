#![allow(missing_docs)]
#![expect(
    clippy::expect_used,
    reason = "benchmark harness uses expect for infallible writes"
)]

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use mantis_fixed::FixedI64;

#[cfg(feature = "bench-fixed-contenders")]
use fixed::types::I38F26 as FixedCrateI38F26;
#[cfg(feature = "bench-fixed-contenders")]
use rust_decimal::Decimal;

type F2 = FixedI64<2>;
type F4 = FixedI64<4>;
type F6 = FixedI64<6>;
type F8 = FixedI64<8>;

fn bench_checked_add(c: &mut Criterion) {
    let mut group = c.benchmark_group("fixed/checked_add");
    let a = F6::from_raw(1_500_000);
    let b = F6::from_raw(2_500_000);
    group.bench_function("FixedI64<6>", |bencher| {
        bencher.iter(|| black_box(a).checked_add(black_box(b)));
    });
    let ra: i64 = 1_500_000;
    let rb: i64 = 2_500_000;
    group.bench_function("raw_i64", |bencher| {
        bencher.iter(|| black_box(ra).checked_add(black_box(rb)));
    });
    group.finish();
}

fn bench_checked_mul_trunc(c: &mut Criterion) {
    let mut group = c.benchmark_group("fixed/checked_mul_trunc");

    let a2 = F2::from_raw(150);
    let b2 = F2::from_raw(200);
    group.bench_function("D=2", |bencher| {
        bencher.iter(|| black_box(a2).checked_mul_trunc(black_box(b2)));
    });
    let a4 = F4::from_raw(15_000);
    let b4 = F4::from_raw(20_000);
    group.bench_function("D=4", |bencher| {
        bencher.iter(|| black_box(a4).checked_mul_trunc(black_box(b4)));
    });
    let a6 = F6::from_raw(1_500_000);
    let b6 = F6::from_raw(2_000_000);
    group.bench_function("D=6", |bencher| {
        bencher.iter(|| black_box(a6).checked_mul_trunc(black_box(b6)));
    });
    let a8 = F8::from_raw(150_000_000);
    let b8 = F8::from_raw(200_000_000);
    group.bench_function("D=8", |bencher| {
        bencher.iter(|| black_box(a8).checked_mul_trunc(black_box(b8)));
    });
    group.finish();
}

fn bench_checked_mul_round_vs_trunc(c: &mut Criterion) {
    let mut group = c.benchmark_group("fixed/mul_round_vs_trunc");
    let a = F6::from_raw(1_500_000);
    let b = F6::from_raw(3_333_333);
    group.bench_function("trunc", |bencher| {
        bencher.iter(|| black_box(a).checked_mul_trunc(black_box(b)));
    });
    group.bench_function("round", |bencher| {
        bencher.iter(|| black_box(a).checked_mul_round(black_box(b)));
    });
    group.finish();
}

fn bench_checked_div(c: &mut Criterion) {
    let mut group = c.benchmark_group("fixed/checked_div");
    let a = F6::from_raw(4_500_000);
    let b = F6::from_raw(1_500_000);
    group.bench_function("trunc", |bencher| {
        bencher.iter(|| black_box(a).checked_div_trunc(black_box(b)));
    });
    group.bench_function("round", |bencher| {
        bencher.iter(|| black_box(a).checked_div_round(black_box(b)));
    });
    group.finish();
}

fn bench_rescale(c: &mut Criterion) {
    let mut group = c.benchmark_group("fixed/rescale");
    let f6 = F6::from_raw(1_555_555);
    group.bench_function("D6_to_D2_trunc", |bencher| {
        bencher.iter(|| black_box(f6).rescale_trunc::<2>());
    });
    let f2 = F2::from_raw(150);
    group.bench_function("D2_to_D8_widen", |bencher| {
        bencher.iter(|| black_box(f2).rescale_trunc::<8>());
    });
    group.finish();
}

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("fixed/parse");
    group.bench_function("short", |bencher| {
        bencher.iter(|| F6::from_str_decimal(black_box("1.5")));
    });
    group.bench_function("full_precision", |bencher| {
        bencher.iter(|| F6::from_str_decimal(black_box("-42.000001")));
    });
    group.bench_function("integer_only", |bencher| {
        bencher.iter(|| F6::from_str_decimal(black_box("123456")));
    });
    group.finish();
}

fn bench_decimal_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("fixed/decimal_parse");

    // Mantis parse_decimal_bytes
    group.bench_function("mantis_bytes/short_0.53", |b| {
        b.iter(|| F6::parse_decimal_bytes(black_box(b"0.53")));
    });
    group.bench_function("mantis_bytes/medium_67396.70", |b| {
        b.iter(|| F2::parse_decimal_bytes(black_box(b"67396.70")));
    });
    group.bench_function("mantis_bytes/long_0.00012345", |b| {
        b.iter(|| F8::parse_decimal_bytes(black_box(b"0.00012345")));
    });
    group.bench_function("mantis_bytes/integer_67396", |b| {
        b.iter(|| F6::parse_decimal_bytes(black_box(b"67396")));
    });

    // Mantis from_str_decimal (for comparison)
    group.bench_function("mantis_str/short_0.53", |b| {
        b.iter(|| F6::from_str_decimal(black_box("0.53")));
    });
    group.bench_function("mantis_str/medium_67396.70", |b| {
        b.iter(|| F2::from_str_decimal(black_box("67396.70")));
    });

    // f64 roundtrip
    #[expect(
        clippy::cast_possible_truncation,
        reason = "benchmark: f64-to-i64 truncation is intentional for roundtrip measurement"
    )]
    group.bench_function("f64_roundtrip/short_0.53", |b| {
        b.iter(|| {
            let f: f64 = black_box("0.53").parse().unwrap_or(0.0);
            F6::from_raw((f * 1_000_000.0) as i64)
        });
    });
    #[expect(
        clippy::cast_possible_truncation,
        reason = "benchmark: f64-to-i64 truncation is intentional for roundtrip measurement"
    )]
    group.bench_function("f64_roundtrip/medium_67396.70", |b| {
        b.iter(|| {
            let f: f64 = black_box("67396.70").parse().unwrap_or(0.0);
            F2::from_raw((f * 100.0) as i64)
        });
    });

    group.finish();
}

fn bench_display(c: &mut Criterion) {
    use std::fmt::Write;
    let mut group = c.benchmark_group("fixed/display");
    let f = F6::from_raw(1_500_000);
    group.bench_function("FixedI64<6>", |bencher| {
        let mut buf = String::with_capacity(32);
        bencher.iter(|| {
            buf.clear();
            write!(&mut buf, "{}", black_box(f)).expect("write to String is infallible");
        });
    });
    group.finish();
}

// --- Contender benchmarks (behind bench-fixed-contenders feature) ---

#[cfg(feature = "bench-fixed-contenders")]
fn bench_contender_add(c: &mut Criterion) {
    let mut group = c.benchmark_group("fixed/contender_add");

    // Mantis
    let ma = F6::from_raw(1_500_000);
    let mb = F6::from_raw(2_500_000);
    group.bench_function("mantis_FixedI64<6>", |bencher| {
        bencher.iter(|| black_box(ma).checked_add(black_box(mb)));
    });

    // rust_decimal
    let da = Decimal::new(150, 2); // 1.50
    let db = Decimal::new(250, 2); // 2.50
    group.bench_function("rust_decimal", |bencher| {
        bencher.iter(|| black_box(da).checked_add(black_box(db)));
    });

    // fixed crate (I38F26 — 38 integer bits, 26 fractional)
    let fa = FixedCrateI38F26::from_num(1.5);
    let fb = FixedCrateI38F26::from_num(2.5);
    group.bench_function("fixed_crate_I38F26", |bencher| {
        bencher.iter(|| black_box(fa).checked_add(black_box(fb)));
    });

    // raw i64
    let ra: i64 = 1_500_000;
    let rb: i64 = 2_500_000;
    group.bench_function("raw_i64", |bencher| {
        bencher.iter(|| black_box(ra).checked_add(black_box(rb)));
    });

    group.finish();
}

#[cfg(feature = "bench-fixed-contenders")]
fn bench_contender_mul(c: &mut Criterion) {
    let mut group = c.benchmark_group("fixed/contender_mul");

    // Mantis (truncating)
    let ma = F6::from_raw(1_500_000); // 1.5
    let mb = F6::from_raw(2_000_000); // 2.0
    group.bench_function("mantis_mul_trunc", |bencher| {
        bencher.iter(|| black_box(ma).checked_mul_trunc(black_box(mb)));
    });

    // rust_decimal
    let da = Decimal::new(150, 2);
    let db = Decimal::new(200, 2);
    group.bench_function("rust_decimal_mul", |bencher| {
        bencher.iter(|| black_box(da).checked_mul(black_box(db)));
    });

    // fixed crate
    let fa = FixedCrateI38F26::from_num(1.5);
    let fb = FixedCrateI38F26::from_num(2.0);
    group.bench_function("fixed_crate_mul", |bencher| {
        bencher.iter(|| black_box(fa).checked_mul(black_box(fb)));
    });

    group.finish();
}

#[cfg(feature = "bench-fixed-contenders")]
fn bench_contender_div(c: &mut Criterion) {
    let mut group = c.benchmark_group("fixed/contender_div");

    // Mantis
    let ma = F6::from_raw(4_500_000); // 4.5
    let mb = F6::from_raw(1_500_000); // 1.5
    group.bench_function("mantis_div_trunc", |bencher| {
        bencher.iter(|| black_box(ma).checked_div_trunc(black_box(mb)));
    });

    // rust_decimal
    let da = Decimal::new(450, 2);
    let db = Decimal::new(150, 2);
    group.bench_function("rust_decimal_div", |bencher| {
        bencher.iter(|| black_box(da).checked_div(black_box(db)));
    });

    // fixed crate
    let fa = FixedCrateI38F26::from_num(4.5);
    let fb = FixedCrateI38F26::from_num(1.5);
    group.bench_function("fixed_crate_div", |bencher| {
        bencher.iter(|| black_box(fa).checked_div(black_box(fb)));
    });

    group.finish();
}

#[cfg(feature = "bench-fixed-contenders")]
fn bench_contender_parse(c: &mut Criterion) {
    use std::str::FromStr;

    let mut group = c.benchmark_group("fixed/contender_parse");

    // Mantis
    group.bench_function("mantis_parse", |bencher| {
        bencher.iter(|| F6::from_str_decimal(black_box("1.500000")));
    });

    // rust_decimal
    group.bench_function("rust_decimal_parse", |bencher| {
        bencher.iter(|| Decimal::from_str(black_box("1.500000")));
    });

    // fixed crate
    group.bench_function("fixed_crate_parse", |bencher| {
        bencher.iter(|| FixedCrateI38F26::from_str(black_box("1.500000")));
    });

    group.finish();
}

#[cfg(feature = "bench-fixed-contenders")]
fn bench_contender_decimal_parse(c: &mut Criterion) {
    use std::str::FromStr;

    let mut group = c.benchmark_group("fixed/contender_decimal_parse");

    let short = "0.53";
    let medium = "67396.70";

    // Mantis (baseline)
    group.bench_function("mantis_bytes/short", |b| {
        b.iter(|| F6::parse_decimal_bytes(black_box(short.as_bytes())));
    });
    group.bench_function("mantis_bytes/medium", |b| {
        b.iter(|| F2::parse_decimal_bytes(black_box(medium.as_bytes())));
    });

    // rust_decimal
    group.bench_function("rust_decimal/short", |b| {
        b.iter(|| Decimal::from_str(black_box(short)));
    });
    group.bench_function("rust_decimal/medium", |b| {
        b.iter(|| Decimal::from_str(black_box(medium)));
    });

    // fixed crate
    group.bench_function("fixed_crate/short", |b| {
        b.iter(|| FixedCrateI38F26::from_str(black_box(short)));
    });
    group.bench_function("fixed_crate/medium", |b| {
        b.iter(|| FixedCrateI38F26::from_str(black_box(medium)));
    });

    // fast-float
    #[expect(
        clippy::cast_possible_truncation,
        reason = "benchmark: f64-to-i64 truncation is intentional"
    )]
    group.bench_function("fast_float/short", |b| {
        b.iter(|| {
            let f: f64 = fast_float2::parse(black_box(short)).unwrap_or(0.0);
            F6::from_raw((f * 1_000_000.0) as i64)
        });
    });
    #[expect(
        clippy::cast_possible_truncation,
        reason = "benchmark: f64-to-i64 truncation is intentional"
    )]
    group.bench_function("fast_float/medium", |b| {
        b.iter(|| {
            let f: f64 = fast_float2::parse(black_box(medium)).unwrap_or(0.0);
            F2::from_raw((f * 100.0) as i64)
        });
    });

    // lexical-core
    #[expect(
        clippy::cast_possible_truncation,
        reason = "benchmark: f64-to-i64 truncation is intentional"
    )]
    group.bench_function("lexical_core/short", |b| {
        b.iter(|| {
            let f: f64 = lexical_core::parse(black_box(short.as_bytes())).unwrap_or(0.0);
            F6::from_raw((f * 1_000_000.0) as i64)
        });
    });
    #[expect(
        clippy::cast_possible_truncation,
        reason = "benchmark: f64-to-i64 truncation is intentional"
    )]
    group.bench_function("lexical_core/medium", |b| {
        b.iter(|| {
            let f: f64 = lexical_core::parse(black_box(medium.as_bytes())).unwrap_or(0.0);
            F2::from_raw((f * 100.0) as i64)
        });
    });

    // stdlib f64 parse
    #[expect(
        clippy::cast_possible_truncation,
        reason = "benchmark: f64-to-i64 truncation is intentional"
    )]
    group.bench_function("stdlib_f64/short", |b| {
        b.iter(|| {
            let f: f64 = black_box(short).parse().unwrap_or(0.0);
            F6::from_raw((f * 1_000_000.0) as i64)
        });
    });
    #[expect(
        clippy::cast_possible_truncation,
        reason = "benchmark: f64-to-i64 truncation is intentional"
    )]
    group.bench_function("stdlib_f64/medium", |b| {
        b.iter(|| {
            let f: f64 = black_box(medium).parse().unwrap_or(0.0);
            F2::from_raw((f * 100.0) as i64)
        });
    });

    group.finish();
}

#[cfg(feature = "bench-fixed-contenders")]
fn bench_contender_display(c: &mut Criterion) {
    use std::fmt::Write;

    let mut group = c.benchmark_group("fixed/contender_display");

    // Mantis
    let mf = F6::from_raw(1_500_000);
    group.bench_function("mantis_display", |bencher| {
        let mut buf = String::with_capacity(32);
        bencher.iter(|| {
            buf.clear();
            write!(&mut buf, "{}", black_box(mf)).expect("infallible");
        });
    });

    // rust_decimal
    let df = Decimal::new(1_500_000, 6);
    group.bench_function("rust_decimal_display", |bencher| {
        let mut buf = String::with_capacity(32);
        bencher.iter(|| {
            buf.clear();
            write!(&mut buf, "{}", black_box(df)).expect("infallible");
        });
    });

    // fixed crate
    let ff = FixedCrateI38F26::from_num(1.5);
    group.bench_function("fixed_crate_display", |bencher| {
        let mut buf = String::with_capacity(32);
        bencher.iter(|| {
            buf.clear();
            write!(&mut buf, "{}", black_box(ff)).expect("infallible");
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_checked_add,
    bench_checked_mul_trunc,
    bench_checked_mul_round_vs_trunc,
    bench_checked_div,
    bench_rescale,
    bench_parse,
    bench_decimal_parse,
    bench_display,
);

#[cfg(feature = "bench-fixed-contenders")]
criterion_group!(
    contender_benches,
    bench_contender_add,
    bench_contender_mul,
    bench_contender_div,
    bench_contender_parse,
    bench_contender_decimal_parse,
    bench_contender_display,
);

#[cfg(feature = "bench-fixed-contenders")]
criterion_main!(benches, contender_benches);
#[cfg(not(feature = "bench-fixed-contenders"))]
criterion_main!(benches);
