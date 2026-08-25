#![allow(dead_code)]

use eyre::Result;
use lunarbase_pmm_math::{
    try_quote_x_to_y_with_multiplier, try_quote_y_to_x_with_multiplier, U256,
};

use crate::canonical_cache::Cache;
use crate::pool_state::PoolState;

#[derive(Debug, Clone)]
pub struct Quote {
    pub amount_out: U256,
    pub fee: U256,
    /// Directional fee used by the current quote before caller multiplier.
    pub effective_fee_x24: u32,
    /// Unchanged Q64.96 operator anchor retained by the Solidity quote ABI.
    pub sqrt_price_next: U256,
    pub head_block: u64,
    pub latest_update_block: u64,
    pub block_age: u64,
    pub fee_multiplier: U256,
    pub caller_whitelisted: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum QuoteError {
    #[error("canonical snapshot is unsynchronized or unavailable")]
    Unhealthy,
    #[error("pool is paused")]
    Paused,
    #[error("price is stale: blockAge={block_age} blockDelay={block_delay}")]
    Stale { block_age: u64, block_delay: u64 },
    #[error("quote rejected (zero anchor output, insufficient reserve, or full effective fee)")]
    Rejected,
}

pub async fn quote_exact_in(cache: &mut Cache, amount_in: U256, x_to_y: bool) -> Result<Quote> {
    let snap = cache
        .snapshot()
        .await
        .map_err(|_| eyre::eyre!("canonical snapshot cache read failed"))?
        .ok_or_else(|| eyre::eyre!(QuoteError::Unhealthy))?;
    quote_from_snapshot(&snap, amount_in, x_to_y)
}

fn quote_from_snapshot(snap: &PoolState, amount_in: U256, x_to_y: bool) -> Result<Quote> {
    if snap.paused {
        return Err(QuoteError::Paused.into());
    }

    let block_age = snap.block_age();

    if !snap.is_fresh() {
        return Err(QuoteError::Stale {
            block_age,
            block_delay: snap.block_delay,
        }
        .into());
    }

    let params = snap.to_params();
    // This is the value that makes the off-chain quote match the Pool's public
    // quote/swap path for the configured execution caller. If the caller is
    // whitelisted it is 1; otherwise it is blacklistFeeMultiplier.
    //
    // Recommendation: run a separate cache/quoter per direct Pool caller. If a
    // partner routes through an intermediate settlement contract, that contract
    // address is the caller to configure here.
    let fee_multiplier = snap.fee_multiplier;
    let result = if x_to_y {
        try_quote_x_to_y_with_multiplier(&params, amount_in, fee_multiplier)
    } else {
        try_quote_y_to_x_with_multiplier(&params, amount_in, fee_multiplier)
    }?;

    if result.amount_out.is_zero() {
        return Err(QuoteError::Rejected.into());
    }

    Ok(Quote {
        amount_out: result.amount_out,
        fee: result.fee,
        effective_fee_x24: result.effective_fee_x24,
        sqrt_price_next: result.sqrt_price_next,
        head_block: snap.snapshot_block,
        latest_update_block: snap.latest_update_block,
        block_age,
        fee_multiplier,
        caller_whitelisted: snap.caller_whitelisted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> PoolState {
        PoolState {
            snapshot_block: 100,
            snapshot_block_hash: alloy::primitives::B256::repeat_byte(0x11),
            sqrt_price_x96: (U256::from(1u64) << 160usize) - U256::from(1u64),
            fee_ask_x24: 0,
            fee_bid_x24: 0,
            latest_update_block: 99,
            reserve_x: (1u128 << 112) - 1,
            reserve_y: (1u128 << 112) - 1,
            max_punishment_x24: 0,
            block_delay: 10,
            paused: false,
            fee_multiplier: U256::from(1u64),
            caller_whitelisted: true,
            blacklist_fee_multiplier: U256::from(2u64),
        }
    }

    #[test]
    fn checked_quote_reports_arithmetic_overflow_instead_of_panicking() {
        let result = quote_from_snapshot(&state(), U256::MAX, true);
        assert!(result.is_err());
    }
}
