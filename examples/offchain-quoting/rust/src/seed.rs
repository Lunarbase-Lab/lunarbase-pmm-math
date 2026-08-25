use alloy::eips::eip1898::RpcBlockHash;
use alloy::primitives::{Address, B256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::{BlockId, BlockTransactionsKind};
use eyre::{ContextCompat, Result};
use tracing::info;

use crate::abi::Pool;
use crate::canonical_cache::Cache;
use crate::pool_state::{parse_decimal_u256, u160_to_u256, PoolState};

#[derive(Clone, Copy, Debug)]
pub struct SnapshotRequest {
    pub pool: Address,
    pub quote_caller: Address,
    pub observed_block: u64,
    pub observed_block_hash: B256,
    pub generation: u64,
    pub connection_epoch: u64,
}

/// Refresh every quote-critical field from the exact confirmed WS head hash
/// and publish it as a single caller-specific Redis snapshot. The caller
/// clears cache health before invoking this function.
pub async fn refresh_canonical_snapshot(
    rpc_url: &str,
    request: SnapshotRequest,
    cache: &mut Cache,
) -> Result<PoolState> {
    let SnapshotRequest {
        pool,
        quote_caller,
        observed_block,
        observed_block_hash,
        generation,
        connection_epoch,
    } = request;
    // Discard transport error sources: reqwest/alloy may include the complete
    // endpoint (and therefore an API key) in Display/Debug output.
    let url = rpc_url
        .parse()
        .map_err(|_| eyre::eyre!("RPC_URL is invalid"))?;
    let provider = ProviderBuilder::new().on_http(url);
    let contract = Pool::new(pool, &provider);

    let rpc_block = provider
        .get_block_by_hash(observed_block_hash, BlockTransactionsKind::Hashes)
        .await
        .map_err(|_| eyre::eyre!("failed to resolve the observed WS head hash through HTTP"))?
        .context("observed WS head hash is unavailable from the HTTP RPC")?;
    if rpc_block.header.hash != observed_block_hash {
        return Err(eyre::eyre!(
            "HTTP RPC returned hash {} for requested WS head hash {}",
            rpc_block.header.hash,
            observed_block_hash
        ));
    }
    if rpc_block.header.number != observed_block {
        return Err(eyre::eyre!(
            "HTTP RPC resolved WS head hash {} to block {}, expected {}",
            observed_block_hash,
            rpc_block.header.number,
            observed_block
        ));
    }

    let snapshot_block = observed_block;
    let snapshot_block_hash = observed_block_hash;
    let block = BlockId::Hash(RpcBlockHash::from_hash(snapshot_block_hash, Some(true)));
    info!(
        %pool,
        %quote_caller,
        snapshot_block,
        %snapshot_block_hash,
        connection_epoch,
        "reading EIP-1898 hash-pinned canonical snapshot"
    );

    let reserve_x: u128 = contract
        .getXReserve()
        .block(block)
        .call()
        .await
        .map_err(|_| eyre::eyre!("getXReserve RPC call failed"))?
        ._0
        .to();
    let reserve_y: u128 = contract
        .getYReserve()
        .block(block)
        .call()
        .await
        .map_err(|_| eyre::eyre!("getYReserve RPC call failed"))?
        ._0
        .to();

    let state = contract
        .state()
        .block(block)
        .call()
        .await
        .map_err(|_| eyre::eyre!("state RPC call failed"))?;
    let max_punishment_x24: u32 = contract
        .maxPunishmentX24()
        .block(block)
        .call()
        .await
        .map_err(|_| eyre::eyre!("maxPunishmentX24 RPC call failed"))?
        ._0
        .to();
    let block_delay: u64 = contract
        .blockDelay()
        .block(block)
        .call()
        .await
        .map_err(|_| eyre::eyre!("blockDelay RPC call failed"))?
        ._0
        .to();
    let paused = contract
        .paused()
        .block(block)
        .call()
        .await
        .map_err(|_| eyre::eyre!("paused RPC call failed"))?
        ._0;

    let caller_whitelisted = contract
        .isWhitelisted(quote_caller)
        .block(block)
        .call()
        .await
        .map_err(|_| eyre::eyre!("isWhitelisted RPC call failed"))?
        ._0;
    let blacklist_multiplier = parse_decimal_u256(
        &contract
            .blacklistFeeMultiplier()
            .block(block)
            .call()
            .await
            .map_err(|_| eyre::eyre!("blacklistFeeMultiplier RPC call failed"))?
            ._0
            .to_string(),
    )
    .context("blacklistFeeMultiplier does not fit U256")?;

    let anchor_price = u160_to_u256(state.anchorPrice);
    let fee_ask_x24: u32 = state.feeAskX24.to();
    let fee_bid_x24: u32 = state.feeBidX24.to();
    let latest_update_block: u64 = state.latestUpdateBlock.to();

    let pool_state = PoolState {
        snapshot_block,
        snapshot_block_hash,
        sqrt_price_x96: anchor_price,
        fee_ask_x24,
        fee_bid_x24,
        latest_update_block,
        reserve_x,
        reserve_y,
        max_punishment_x24,
        block_delay,
        paused,
        fee_multiplier: if caller_whitelisted {
            lunarbase_pmm_math::U256::from(1u64)
        } else {
            blacklist_multiplier
        },
        caller_whitelisted,
        blacklist_fee_multiplier: blacklist_multiplier,
    };
    pool_state
        .to_params()
        .validate()
        .map_err(|error| eyre::eyre!("RPC returned invalid pool parameters: {error}"))?;
    if blacklist_multiplier.is_zero() {
        return Err(eyre::eyre!("RPC returned zero blacklistFeeMultiplier"));
    }

    cache
        .publish_snapshot(&pool_state, generation, connection_epoch)
        .await
        .map_err(|_| eyre::eyre!("failed to publish canonical snapshot"))?;

    info!(
        snapshot_block,
        %snapshot_block_hash,
        connection_epoch,
        quote_caller = %quote_caller,
        reserve_x,
        reserve_y,
        anchor_price = %anchor_price,
        fee_ask_x24,
        fee_bid_x24,
        latest_update_block,
        max_punishment_x24,
        block_delay,
        paused,
        caller_whitelisted,
        blacklist_fee_multiplier = %blacklist_multiplier,
        "published canonical pool snapshot"
    );

    Ok(pool_state)
}
