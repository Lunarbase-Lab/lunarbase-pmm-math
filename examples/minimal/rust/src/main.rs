//! Minimal example: quote a swap in both directions and print the results.
//!
//! Run from the repo root: `cargo run --manifest-path examples/rust/Cargo.toml`

use lunarbase_pmm_math::{quote_x_to_y, quote_y_to_x, PoolParams, U256};

fn main() {
    let params = PoolParams {
        sqrt_price_x48: 1u128 << 48,
        anchor_sqrt_price_x48: 1u128 << 48,
        fee_q48: 1u64 << 44,
        reserve_x: 1_000_000_000,
        reserve_y: 1_000_000_000,
        concentration_k: 5_000,
    };

    let dx = U256::from(10_000u64);
    let r = quote_x_to_y(&params, dx);
    println!(
        "X->Y  in={dx}  out={}  fee={}  pNext={}",
        r.amount_out, r.fee, r.sqrt_price_next
    );

    let dy = U256::from(10_000u64);
    let r = quote_y_to_x(&params, dy);
    println!(
        "Y->X  in={dy}  out={}  fee={}  pNext={}",
        r.amount_out, r.fee, r.sqrt_price_next
    );
}
