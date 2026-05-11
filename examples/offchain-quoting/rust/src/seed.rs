use alloy::primitives::Address;
use alloy::providers::{Provider, ProviderBuilder};
use eyre::{Context, Result};
use lunarbase_pmm_math::U256 as PmmU256;
use tracing::info;

use crate::abi::Pool;
use crate::cache::Cache;

pub async fn seed_state(rpc_url: &str, pool: Address, cache: &mut Cache) -> Result<()> {
    let url = rpc_url.parse().context("bad RPC_URL")?;
    let provider = ProviderBuilder::new().on_http(url);
    let contract = Pool::new(pool, &provider);

    let head_block = provider.get_block_number().await?;
    cache.set_head_block(head_block).await?;

    let reserve_x: u128 = contract.getXReserve().call().await?._0.to();
    let reserve_y: u128 = contract.getYReserve().call().await?._0.to();
    let state = contract.state().call().await?;
    let k = contract.concentrationK().call().await?._0;
    let delay: u64 = contract.blockDelay().call().await?._0.to();
    let paused = contract.paused().call().await?._0;

    // anchorPrice is uint160 (Q64.96 sqrt-price). Convert via decimal string to
    // PmmU256 so the cache stores it at full precision.
    let anchor_price = PmmU256::from_str_radix(&state.anchorPrice.to_string(), 10)
        .context("parse anchorPrice as U256")?;
    let fee_ask_x24: u32 = state.feeAskX24.to();
    let fee_bid_x24: u32 = state.feeBidX24.to();
    let latest_update_block: u64 = state.latestUpdateBlock.to();

    cache.set_reserves(reserve_x, reserve_y).await?;
    cache
        .set_state(latest_update_block, anchor_price, fee_ask_x24, fee_bid_x24)
        .await?;
    cache.set_sqrt_price(anchor_price).await?;
    cache.set_concentration_k(k).await?;
    cache.set_block_delay(delay).await?;
    cache.set_paused(paused).await?;

    info!(
        head_block,
        reserve_x,
        reserve_y,
        anchor_price = %anchor_price,
        fee_ask_x24,
        fee_bid_x24,
        latest_update_block,
        concentration_k = k,
        block_delay = delay,
        paused,
        "seeded pool state from RPC"
    );

    Ok(())
}
