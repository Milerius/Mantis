use criterion::{Criterion, black_box, criterion_group, criterion_main};
use mantis_fixed::FixedI64;

type F2 = FixedI64<2>;
type F4 = FixedI64<4>;
type F6 = FixedI64<6>;
type F8 = FixedI64<8>;

fn bench_checked_add(c: &mut Criterion) {
    let mut group = c.benchmark_group("checked_add");
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
    let mut group = c.benchmark_group("checked_mul_trunc");

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
    let mut group = c.benchmark_group("mul_round_vs_trunc");
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
    let mut group = c.benchmark_group("checked_div");
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
    let mut group = c.benchmark_group("rescale");
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
    let mut group = c.benchmark_group("parse");
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

fn bench_display(c: &mut Criterion) {
    use std::fmt::Write;
    let mut group = c.benchmark_group("display");
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

criterion_group!(
    benches,
    bench_checked_add,
    bench_checked_mul_trunc,
    bench_checked_mul_round_vs_trunc,
    bench_checked_div,
    bench_rescale,
    bench_parse,
    bench_display,
);
criterion_main!(benches);
