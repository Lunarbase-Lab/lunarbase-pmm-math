mod swap;

use alloy::primitives::{keccak256, B256};
use alloy::sol_types::SolEvent;
use eyre::{ContextCompat, Result};
use tracing::{debug, info};

use crate::abi::Pool;
use crate::cache::Cache;
use crate::pool_state::{parse_decimal_u256, u160_to_u128_checked};
use crate::ws::types::LogEvent;
use crate::ws::ChainEvent;

pub async fn dispatch(event: ChainEvent, cache: &mut Cache) -> Result<()> {
    match event {
        ChainEvent::Head { number } => {
            cache.set_head_block(number).await?;
            debug!(block = number, "head");
        }
        ChainEvent::Flashblock { number } => {
            debug!(block = number, "flashblock");
        }
        ChainEvent::Log(log) => {
            handle_log(log, cache).await?;
        }
    }
    Ok(())
}

async fn handle_log(log: LogEvent, cache: &mut Cache) -> Result<()> {
    let block = log.block_number.unwrap_or(0);
    let log_idx = log.log_index.unwrap_or(0);
    let fingerprint = log_fingerprint(&log);
    if !cache.try_take_log(&fingerprint).await? {
        debug!(block, log_idx, fp = %fingerprint, "duplicate log skipped");
        return Ok(());
    }

    let Some(topic0) = log.topics.first().copied() else {
        return Ok(());
    };

    if topic0 == sig::<Pool::StateUpdated>() {
        let ev = decode::<Pool::StateUpdated>(&log)?;
        let anchor = u160_to_u128_checked(ev.anchorPrice)
            .context("anchorPrice exceeds the math crate's supported u128 Q96 range")?;
        let fee_ask: u32 = ev.feeAskX24.to();
        let fee_bid: u32 = ev.feeBidX24.to();
        cache.set_state(block, anchor, fee_ask, fee_bid).await?;
        info!(
            block,
            anchor_price = %anchor,
            fee_ask_x24 = fee_ask,
            fee_bid_x24 = fee_bid,
            "StateUpdated"
        );
    } else if topic0 == sig::<Pool::Sync>() {
        let ev = decode::<Pool::Sync>(&log)?;
        let x: u128 = ev.reserveX;
        let y: u128 = ev.reserveY;
        cache.set_reserves(x, y).await?;
        info!(block, reserve_x = x, reserve_y = y, "Sync");
    } else if topic0 == sig::<Pool::SwapExecuted>() {
        let ev = decode::<Pool::SwapExecuted>(&log)?;
        // Current pools emit an authoritative Sync before SwapExecuted. The
        // hypothetical pNext returned by quote math is not persisted on-chain,
        // so applying the swap again here would double-update the reserves.
        swap::observe(&ev);
    } else if topic0 == sig::<Pool::ConcentrationKSet>() {
        let ev = decode::<Pool::ConcentrationKSet>(&log)?;
        cache.set_concentration_k(ev.concentrationK).await?;
        info!(concentration_k = ev.concentrationK, "ConcentrationKSet");
    } else if topic0 == sig::<Pool::BlockDelaySet>() {
        let ev = decode::<Pool::BlockDelaySet>(&log)?;
        let d: u64 = ev.blockDelay.to();
        cache.set_block_delay(d).await?;
        info!(block_delay = d, "BlockDelaySet");
    } else if topic0 == sig::<Pool::WhitelistSet>() {
        let ev = decode::<Pool::WhitelistSet>(&log)?;
        if cache.is_quote_caller(ev.account) {
            // Whitelist is keyed by msg.sender. Only update this quote cache
            // when the event targets the exact router/adapter address that
            // production swaps use to call the Pool.
            cache.set_caller_whitelisted(ev.whitelisted).await?;
            info!(
                account = %ev.account,
                whitelisted = ev.whitelisted,
                "WhitelistSet for quote caller"
            );
        } else {
            debug!(
                account = %ev.account,
                whitelisted = ev.whitelisted,
                "WhitelistSet ignored for non-quote caller"
            );
        }
    } else if topic0 == sig::<Pool::BlacklistFeeMultiplierSet>() {
        let ev = decode::<Pool::BlacklistFeeMultiplierSet>(&log)?;
        let multiplier = parse_decimal_u256(&ev.multiplier.to_string())
            .context("BlacklistFeeMultiplierSet multiplier does not fit U256")?;
        cache.set_blacklist_fee_multiplier(multiplier).await?;
        info!(multiplier = %multiplier, "BlacklistFeeMultiplierSet");
    } else if topic0 == sig::<Pool::Paused>() {
        cache.set_paused(true).await?;
        info!("Paused");
    } else if topic0 == sig::<Pool::Unpaused>() {
        cache.set_paused(false).await?;
        info!("Unpaused");
    } else {
        debug!(?topic0, "ignored topic");
    }

    Ok(())
}

fn sig<E: SolEvent>() -> B256 {
    E::SIGNATURE_HASH
}

fn decode<E: SolEvent>(log: &LogEvent) -> Result<E> {
    let topics = log.topics.to_vec();
    Ok(E::decode_raw_log(topics, &log.data, true)?)
}

/// Stable identifier for a log irrespective of how many times the flashblocks
/// node re-emits it.
///
/// pendingLogs from a flashblocks-aware node reassign `logIndex` between
/// successive pre-confirmation snapshots of the same block, so `(block,
/// logIndex)` alone produces duplicates. We prefer `(block, txHash, logIndex)`
/// when the node ships a tx hash, and fall back to keccak over the immutable
/// payload (block ‖ topic0 ‖ data) otherwise.
fn log_fingerprint(log: &LogEvent) -> String {
    let block = log.block_number.unwrap_or(0);
    if let Some(hash) = log.transaction_hash {
        return format!("{:x}:{:x}:{}", block, hash, log.log_index.unwrap_or(0));
    }
    let mut payload = Vec::with_capacity(8 + 32 + log.data.len());
    payload.extend_from_slice(&block.to_be_bytes());
    if let Some(t0) = log.topics.first() {
        payload.extend_from_slice(t0.as_slice());
    }
    payload.extend_from_slice(&log.data);
    let digest = keccak256(&payload);
    format!("{:x}:{:x}", block, digest)
}
