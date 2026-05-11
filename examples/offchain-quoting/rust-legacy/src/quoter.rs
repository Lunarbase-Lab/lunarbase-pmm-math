#![allow(dead_code)]

use eyre::Result;
use lunarbase_pmm_math::{quote_x_to_y, quote_y_to_x, U256};

use crate::cache::Cache;

#[derive(Debug, Clone)]
pub struct Quote {
    pub amount_out: U256,
    pub fee: U256,
    /// Q64.96 sqrt-price the swap would settle at. Informational only on
    /// the current math layer.
    pub sqrt_price_next: U256,
    pub head_block: u64,
    pub latest_update_block: u64,
    pub block_age: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum QuoteError {
    #[error("pool state not yet seeded")]
    NoState,
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

    let head = cache.get_head_block().await?.unwrap_or(0);
    let block_age = head.saturating_sub(snap.latest_update_block);

    if !snap.is_fresh(head) {
        return Err(QuoteError::Stale {
            block_age,
            block_delay: snap.block_delay,
        }
        .into());
    }

    let params = snap.to_params();
    let result = if x_to_y {
        quote_x_to_y(&params, dx)
    } else {
        quote_y_to_x(&params, dx)
    };

    if result.amount_out.is_zero() && result.fee.is_zero() {
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
