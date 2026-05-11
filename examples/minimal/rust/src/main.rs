//! Minimal example: quote a swap in both directions and print the results.
//!
//! Run from the repo root:
//!   cargo run --manifest-path examples/minimal/rust/Cargo.toml

use lunarbase_pmm_math::{quote_x_to_y, quote_y_to_x, PoolParams, U256Ext, U256};

fn main() {
    let params = PoolParams {
        // Q64.96 = 2^96 represents price = 1.0.
        sqrt_price_x96: U256::Q96,
        // 0.10% fees in Q24 (Q24 = 2^24 = 100%).
        fee_ask_x24: (1u32 << 24) / 1000,
        fee_bid_x24: (1u32 << 24) / 1000,
        reserve_x: 1_000_000_000,
        reserve_y: 1_000_000_000,
        // Legacy plain-int K=5000 maps to 5000 << 12 in Q20.12.
        concentration_k: 5_000 << 12,
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
