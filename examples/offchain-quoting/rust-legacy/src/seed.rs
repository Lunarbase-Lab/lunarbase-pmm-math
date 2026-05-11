use alloy::primitives::Address;
use alloy::providers::{Provider, ProviderBuilder};
use eyre::{Context, Result};
use tracing::info;

use crate::abi::Pool;
use crate::cache::Cache;
use crate::pool_state::px96_to_u256;

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

    let p_x96 = state.pX96;
    let p_x96_u = px96_to_u256(p_x96);
    let fee_q48: u64 = state.fee.to();
    let latest_update_block: u64 = state.latestUpdateBlock.to();

    // Legacy contract has no separate `anchorPrice()` view: operator-published
    // sqrt-price IS the anchor on every StateUpdated, and it also resets the
    // current sqrt-price.
    let anchor_px96 = p_x96_u;

    cache.set_reserves(reserve_x, reserve_y).await?;
    cache
        .set_state(latest_update_block, anchor_px96, fee_q48)
        .await?;
    cache.set_sqrt_price(p_x96_u).await?;
    cache.set_concentration_k(k).await?;
    cache.set_block_delay(delay).await?;
    cache.set_paused(paused).await?;

    info!(
        head_block,
        reserve_x,
        reserve_y,
        %p_x96,
        anchor_px96 = %anchor_px96,
        fee_q48,
        latest_update_block,
        concentration_k = k,
        block_delay = delay,
        paused,
        "seeded pool state from RPC (legacy Q96 contract)"
    );

    Ok(())
}
