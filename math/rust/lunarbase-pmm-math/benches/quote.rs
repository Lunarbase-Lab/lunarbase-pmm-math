//! Benchmarks for `quote_x_to_y` / `quote_y_to_x`.
//!
//! Run with `cargo bench -p lunarbase-pmm-math`. Five scenarios per direction,
//! ten benchmarks total. The same scenarios are mirrored in the Go bench
//! suite (`math/go/quote_bench_test.go`) so cross-language comparisons are
//! apples-to-apples.
#![allow(missing_docs)] // criterion_group! generates a pub mod that's not documented

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use lunarbase_pmm_math::{quote_x_to_y, quote_y_to_x, PoolParams, U256};

const Q48: u128 = 1u128 << 48;
const Q24: u32 = 1u32 << 24;

fn symmetric_pool() -> PoolParams {
    PoolParams {
        sqrt_price_x48: Q48,
        anchor_sqrt_price_x48: Q48,
        fee_ask_x24: Q24 / 1_000, // 0.10%
        fee_bid_x24: Q24 / 1_000, // 0.10%
        reserve_x: 1_000_000_000_000_000_000,
        reserve_y: 1_000_000_000_000_000_000,
        concentration_k_q12: 5_000,
    }
}

fn asymmetric_pool() -> PoolParams {
    let p = (Q48 * 3) / 2; // 1.5x
    PoolParams {
        sqrt_price_x48: p,
        anchor_sqrt_price_x48: p,
        fee_ask_x24: Q24 / 100, // 1.00%
        fee_bid_x24: Q24 / 333, // ~0.30%
        reserve_x: 750_000_000_000_000_000,
        reserve_y: 1_500_000_000_000_000_000,
        concentration_k_q12: 8_000,
    }
}

fn bench_quotes(c: &mut Criterion) {
    let sym = symmetric_pool();
    let asym = asymmetric_pool();

    let mid = U256::from(10_000_000_000_000_000u128);
    let near_bound = U256::from(900_000_000_000_000_000u128);
    let tiny = U256::from(1u64);
    let too_large = U256::from(10_000_000_000_000_000_000u128);

    let mut g = c.benchmark_group("quote_x_to_y");
    g.bench_function("symmetric_mid", |b| {
        b.iter(|| quote_x_to_y(black_box(&sym), black_box(mid)).amount_out);
    });
    g.bench_function("near_bound", |b| {
        b.iter(|| quote_x_to_y(black_box(&sym), black_box(near_bound)).amount_out);
    });
    g.bench_function("tiny_amount", |b| {
        b.iter(|| quote_x_to_y(black_box(&sym), black_box(tiny)).amount_out);
    });
    g.bench_function("rejected_too_large", |b| {
        b.iter(|| quote_x_to_y(black_box(&sym), black_box(too_large)).amount_out);
    });
    g.bench_function("asymmetric_pool", |b| {
        b.iter(|| quote_x_to_y(black_box(&asym), black_box(mid)).amount_out);
    });
    g.finish();

    let mut g = c.benchmark_group("quote_y_to_x");
    g.bench_function("symmetric_mid", |b| {
        b.iter(|| quote_y_to_x(black_box(&sym), black_box(mid)).amount_out);
    });
    g.bench_function("near_bound", |b| {
        b.iter(|| quote_y_to_x(black_box(&sym), black_box(near_bound)).amount_out);
    });
    g.bench_function("tiny_amount", |b| {
        b.iter(|| quote_y_to_x(black_box(&sym), black_box(tiny)).amount_out);
    });
    g.bench_function("rejected_too_large", |b| {
        b.iter(|| quote_y_to_x(black_box(&sym), black_box(too_large)).amount_out);
    });
    g.bench_function("asymmetric_pool", |b| {
        b.iter(|| quote_y_to_x(black_box(&asym), black_box(mid)).amount_out);
    });
    g.finish();
}

criterion_group!(benches, bench_quotes);
criterion_main!(benches);
