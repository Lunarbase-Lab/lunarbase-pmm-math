use alloy::primitives::Address;
use alloy::providers::{Provider, ProviderBuilder};
use eyre::{Context, Result};
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
    let k = contract.concentrationKQ12().call().await?._0;
    let delay: u64 = contract.blockDelay().call().await?._0.to();
    let paused = contract.paused().call().await?._0;

    let anchor_price: u128 = state.anchorPrice.to();
    let p_x48: u128 = state.pX48.to();
    let fee_ask_x24: u32 = state.feeAskX24.to();
    let fee_bid_x24: u32 = state.feeBidX24.to();
    let latest_update_block: u64 = state.latestUpdateBlock.to();

    cache.set_reserves(reserve_x, reserve_y).await?;
    cache
        .set_state(latest_update_block, anchor_price, fee_ask_x24, fee_bid_x24)
        .await?;
    cache.set_sqrt_price(p_x48).await?;
    cache.set_concentration_k_q12(k).await?;
    cache.set_block_delay(delay).await?;
    cache.set_paused(paused).await?;

    info!(
        head_block,
        reserve_x,
        reserve_y,
        anchor_price,
        fee_ask_x24,
        fee_bid_x24,
        p_x48,
        latest_update_block,
        concentration_k_q12 = k,
        block_delay = delay,
        paused,
        "seeded pool state from RPC"
    );

    Ok(())
}
