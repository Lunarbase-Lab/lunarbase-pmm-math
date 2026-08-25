use std::time::Duration;

use alloy::primitives::Address;
use eyre::{Context, Result};
use tracing::{debug, info};

use crate::canonical_cache::Cache;
use crate::seed;
use crate::ws::{ActiveConnectionEpoch, ChainEvent, ConnectionEvent};

pub async fn dispatch(
    connection_event: ConnectionEvent,
    cache: &mut Cache,
    rpc_url: &str,
    pool: Address,
    quote_caller: Address,
    refresh_timeout: Duration,
    active_epoch: &ActiveConnectionEpoch,
) -> Result<()> {
    let ConnectionEvent {
        epoch,
        generation,
        event,
    } = connection_event;
    match event {
        ChainEvent::Connected => {
            if !active_epoch.is_active(epoch) {
                debug!(connection_epoch = epoch, "ignoring stale Connected event");
                return Ok(());
            }
            info!(
                connection_epoch = epoch,
                "confirmed-head subscription ready; quotes remain disabled until the first valid Head"
            );
        }
        ChainEvent::Disconnected => {
            active_epoch.invalidate(epoch);
            let _ = cache
                .disconnect_connection_epoch(epoch)
                .await
                .map_err(|_| eyre::eyre!("failed to invalidate quote cache after disconnect"))?;
            info!(connection_epoch = epoch, "WebSocket disconnected; quotes disabled until a new connection's Head snapshot succeeds");
        }
        ChainEvent::Head { number, hash } => {
            let Some(generation) = generation else {
                return Err(eyre::eyre!("Head event has no snapshot generation"));
            };
            if !active_epoch.is_current_head(epoch, generation) {
                debug!(connection_epoch = epoch, generation, number, %hash, "ignoring queued or superseded Head");
                return Ok(());
            }
            debug!(
                connection_epoch = epoch,
                generation,
                observed_block = number,
                observed_block_hash = %hash,
                "confirmed head; refreshing hash-pinned canonical snapshot"
            );
            let request = seed::SnapshotRequest {
                pool,
                quote_caller,
                observed_block: number,
                observed_block_hash: hash,
                generation,
                connection_epoch: epoch,
            };
            tokio::time::timeout(
                refresh_timeout,
                seed::refresh_canonical_snapshot(rpc_url, request, cache),
            )
            .await
            .context("timed out while refreshing the canonical pool snapshot")??;
        }
    }
    Ok(())
}
