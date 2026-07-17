use alloy::primitives::Address;
use alloy::providers::{Provider, ProviderBuilder};
use eyre::{Context, ContextCompat, Result};
use tracing::info;

use crate::abi::Pool;
use crate::cache::Cache;
use crate::pool_state::{parse_decimal_u256, u160_to_u128_checked};

pub async fn seed_state(
    rpc_url: &str,
    pool: Address,
    quote_caller: Address,
    cache: &mut Cache,
) -> Result<()> {
    let url = rpc_url.parse().context("bad RPC_URL")?;
    let provider = ProviderBuilder::new().on_http(url);
    let contract = Pool::new(pool, &provider);

    info!(%rpc_url, %pool, %quote_caller, "seed: reading head block");
    let head_block = provider.get_block_number().await?;
    cache.set_head_block(head_block).await?;

    info!(head_block, "seed: reading reserves");
    let reserve_x: u128 = contract.getXReserve().call().await?._0.to();
    let reserve_y: u128 = contract.getYReserve().call().await?._0.to();

    info!(
        head_block,
        reserve_x, reserve_y, "seed: reading pool parameters"
    );
    let state = contract.state().call().await?;
    let k = contract.concentrationK().call().await?._0;
    let delay: u64 = contract.blockDelay().call().await?._0.to();
    let paused = contract.paused().call().await?._0;

    info!(%quote_caller, "seed: reading caller fee policy");
    let caller_whitelisted = contract.isWhitelisted(quote_caller).call().await?._0;
    let blacklist_multiplier = parse_decimal_u256(
        &contract
            .blacklistFeeMultiplier()
            .call()
            .await?
            ._0
            .to_string(),
    )
    .context("blacklistFeeMultiplier does not fit U256")?;

    let anchor_price = u160_to_u128_checked(state.anchorPrice)
        .context("anchorPrice exceeds the math crate's supported u128 Q96 range")?;
    let fee_ask_x24: u32 = state.feeAskX24.to();
    let fee_bid_x24: u32 = state.feeBidX24.to();
    let latest_update_block: u64 = state.latestUpdateBlock.to();

    cache.set_reserves(reserve_x, reserve_y).await?;
    cache
        .set_state(latest_update_block, anchor_price, fee_ask_x24, fee_bid_x24)
        .await?;
    cache.set_concentration_k(k).await?;
    cache.set_block_delay(delay).await?;
    cache.set_paused(paused).await?;
    cache
        .set_fee_policy(caller_whitelisted, blacklist_multiplier)
        .await?;

    info!(
        head_block,
        quote_caller = %quote_caller,
        reserve_x,
        reserve_y,
        anchor_price = %anchor_price,
        fee_ask_x24,
        fee_bid_x24,
        latest_update_block,
        concentration_k = k,
        block_delay = delay,
        paused,
        caller_whitelisted,
        blacklist_fee_multiplier = %blacklist_multiplier,
        "seeded pool state from RPC"
    );

    Ok(())
}
