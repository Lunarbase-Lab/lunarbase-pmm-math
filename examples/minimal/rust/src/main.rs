//! Minimal example: quote a swap in both directions and print the results.
//!
//! Run from the repo root:
//!   cargo run --manifest-path examples/minimal/rust/Cargo.toml

use lunarbase_pmm_math::{
    quote_x_to_y_with_multiplier, quote_y_to_x_with_multiplier, simulate_successful_swap,
    Direction, PoolParams, Q24, U256,
};

fn main() {
    let params = PoolParams {
        // Q64.96 = 2^96 represents price = 1.0.
        sqrt_price_x96: U256::from(1u64) << 96,
        // 0.10% fees in Q24 (Q24 = 2^24 = 100%).
        fee_ask_x24: (1u32 << 24) / 1000,
        fee_bid_x24: (1u32 << 24) / 1000,
        reserve_x: 1_000_000_000,
        reserve_y: 1_000_000_000,
        // A full-inventory swap can add at most 10% to its direction's fee.
        max_punishment_x24: Q24 / 10,
    };

    // Aggregator/execution-adapter callers are whitelisted and therefore use
    // the Pool's base fee without the blacklist multiplier.
    let fee_multiplier = U256::from(1u64);
    let dx = U256::from(10_000u64);
    let r = quote_x_to_y_with_multiplier(&params, dx, fee_multiplier);
    println!(
        "X->Y  in={dx}  out={}  fee={}  effectiveFee={}  pNext={}",
        r.amount_out, r.fee, r.effective_fee_x24, r.sqrt_price_next
    );
    let simulation =
        simulate_successful_swap(&params, dx, Direction::XToY, fee_multiplier);
    println!(
        "      desiredPunishment={} appliedPunishment={} nextBidFee={}",
        simulation.punishment.desired_punishment_x24,
        simulation.punishment.applied_punishment_x24,
        simulation.post_swap.fee_bid_x24,
    );

    let dy = U256::from(10_000u64);
    let r = quote_y_to_x_with_multiplier(&params, dy, fee_multiplier);
    println!(
        "Y->X  in={dy}  out={}  fee={}  effectiveFee={}  pNext={}",
        r.amount_out, r.fee, r.effective_fee_x24, r.sqrt_price_next
    );
}
