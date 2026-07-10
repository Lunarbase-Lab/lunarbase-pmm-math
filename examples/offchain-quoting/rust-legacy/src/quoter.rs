#![allow(dead_code)]

use eyre::Result;
use lunarbase_pmm_math::{quote_x_to_y_with_multiplier, quote_y_to_x_with_multiplier, U256};

use crate::cache::Cache;

const WHITELIST_FEE_MULTIPLIER: u64 = 1;

#[derive(Debug, Clone)]
pub struct Quote {
    pub amount_out: U256,
    pub fee: U256,
    /// Q64.96 sqrt-price the swap would settle at. Informational only on
    /// the current math layer.
    pub sqrt_price_next: u128,
    pub head_block: u64,
    pub latest_update_block: u64,
    pub block_age: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum QuoteError {
    #[error("pool state not yet seeded")]
    NoState,
    #[error("head block unavailable; refusing to quote against unverifiable freshness")]
    NoHead,
    #[error("pool is paused")]
    Paused,
    #[error("price is stale: blockAge={block_age} blockDelay={block_delay}")]
    Stale { block_age: u64, block_delay: u64 },
    #[error("quote rejected by curve (no liquidity within bounds)")]
    Rejected,
}

pub async fn quote_exact_in(cache: &mut Cache, dx: U256, x_to_y: bool) -> Result<Quote> {
    let snap = cache
        .snapshot()
        .await?
        .ok_or_else(|| eyre::eyre!(QuoteError::NoState))?;

    if snap.paused {
        return Err(QuoteError::Paused.into());
    }

    let head = cache
        .get_head_block()
        .await?
        .ok_or_else(|| eyre::eyre!(QuoteError::NoHead))?;
    let block_age = head.saturating_sub(snap.latest_update_block);

    if !snap.is_fresh(head) {
        return Err(QuoteError::Stale {
            block_age,
            block_delay: snap.block_delay,
        }
        .into());
    }

    let params = snap.to_params();
    // Legacy partner routes shown by this example are whitelisted.
    let fee_multiplier = U256::from(WHITELIST_FEE_MULTIPLIER);
    let result = if x_to_y {
        quote_x_to_y_with_multiplier(&params, dx, fee_multiplier)
    } else {
        quote_y_to_x_with_multiplier(&params, dx, fee_multiplier)
    };

    if result.amount_out.is_zero() {
        return Err(QuoteError::Rejected.into());
    }

    Ok(Quote {
        amount_out: result.amount_out,
        fee: result.fee,
        sqrt_price_next: result.sqrt_price_next,
        head_block: head,
        latest_update_block: snap.latest_update_block,
        block_age,
    })
}
