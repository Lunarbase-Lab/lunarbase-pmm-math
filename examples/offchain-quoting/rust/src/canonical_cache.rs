use alloy::primitives::Address;
use eyre::{Context, Result};
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

use crate::pool_state::{CanonicalSnapshotPayload, PoolState};

/// A short lease ensures a stalled head subscription cannot leave quotes
/// healthy indefinitely. Every canonical snapshot publication renews it.
const SYNCHRONIZED_TTL_SECS: u64 = 30;

pub struct Cache {
    snapshot_key: String,
    synchronized_key: String,
    generation_key: String,
    connection_epoch_counter_key: String,
    active_connection_epoch_key: String,
    conn: ConnectionManager,
}

impl Cache {
    pub async fn connect(redis_url: &str, pool: Address, quote_caller: Address) -> Result<Self> {
        let client = redis::Client::open(redis_url).context("invalid REDIS_URL")?;
        let conn = ConnectionManager::new(client)
            .await
            .context("failed to connect to Redis")?;
        let namespace = format!("{pool:#x}:{quote_caller:#x}");
        Ok(Self {
            snapshot_key: format!("pmm:canonicalSnapshot:{namespace}"),
            synchronized_key: format!("pmm:synchronized:{namespace}"),
            generation_key: format!("pmm:snapshotGeneration:{namespace}"),
            connection_epoch_counter_key: format!("pmm:connectionEpochCounter:{namespace}"),
            active_connection_epoch_key: format!("pmm:activeConnectionEpoch:{namespace}"),
            conn,
        })
    }

    /// Start a new WebSocket connection epoch and invalidate every snapshot
    /// from an older connection in one Redis operation. The returned epoch is
    /// attached to every event produced by that connection.
    pub async fn begin_connection_epoch(&mut self) -> Result<u64> {
        let epoch: i64 = redis::Script::new(
            r#"
                local epoch = redis.call('INCR', KEYS[1])
                redis.call('SET', KEYS[2], epoch)
                redis.call('INCR', KEYS[3])
                redis.call('SET', KEYS[4], '0', 'EX', ARGV[1])
                return epoch
            "#,
        )
        .key(&self.connection_epoch_counter_key)
        .key(&self.active_connection_epoch_key)
        .key(&self.generation_key)
        .key(&self.synchronized_key)
        .arg(SYNCHRONIZED_TTL_SECS)
        .invoke_async(&mut self.conn)
        .await?;
        u64::try_from(epoch).context("Redis connection epoch is negative")
    }

    /// Atomically retire one connection epoch and invalidate any snapshot read
    /// that it may still have in flight. A late disconnect from an older epoch
    /// cannot invalidate the current connection.
    pub async fn disconnect_connection_epoch(&mut self, epoch: u64) -> Result<bool> {
        let invalidated: i32 = redis::Script::new(
            r#"
                if redis.call('GET', KEYS[1]) ~= ARGV[1] then
                    return 0
                end
                redis.call('DEL', KEYS[1])
                redis.call('INCR', KEYS[2])
                redis.call('SET', KEYS[3], '0', 'EX', ARGV[2])
                return 1
            "#,
        )
        .key(&self.active_connection_epoch_key)
        .key(&self.generation_key)
        .key(&self.synchronized_key)
        .arg(epoch.to_string())
        .arg(SYNCHRONIZED_TTL_SECS)
        .invoke_async(&mut self.conn)
        .await?;
        Ok(invalidated == 1)
    }

    /// Begin a snapshot read only while its originating WebSocket connection
    /// remains active. The epoch check, generation increment, and health
    /// invalidation are one atomic Redis operation, so queued heads from a
    /// disconnected connection cannot supersede the disconnect generation.
    pub async fn begin_snapshot_refresh(&mut self, epoch: u64) -> Result<Option<u64>> {
        let (active, generation): (i32, i64) = redis::Script::new(
            r#"
                if redis.call('GET', KEYS[1]) ~= ARGV[1] then
                    return {0, 0}
                end
                local generation = redis.call('INCR', KEYS[2])
                redis.call('SET', KEYS[3], '0', 'EX', ARGV[2])
                return {1, generation}
            "#,
        )
        .key(&self.active_connection_epoch_key)
        .key(&self.generation_key)
        .key(&self.synchronized_key)
        .arg(epoch.to_string())
        .arg(SYNCHRONIZED_TTL_SECS)
        .invoke_async(&mut self.conn)
        .await?;
        if active != 1 {
            return Ok(None);
        }
        Ok(Some(
            u64::try_from(generation).context("Redis snapshot generation is negative")?,
        ))
    }

    /// Fail closed before any RPC work begins. If a subsequent pinned read or
    /// Redis publication fails, this key remains false (and also expires).
    pub async fn mark_unsynchronized(&mut self) -> Result<u64> {
        let generation: i64 = redis::Script::new(
            r#"
                local generation = redis.call('INCR', KEYS[1])
                redis.call('SET', KEYS[2], '0', 'EX', ARGV[1])
                return generation
            "#,
        )
        .key(&self.generation_key)
        .key(&self.synchronized_key)
        .arg(SYNCHRONIZED_TTL_SECS)
        .invoke_async(&mut self.conn)
        .await?;
        u64::try_from(generation).context("Redis snapshot generation is negative")
    }

    /// Publish the complete caller-specific snapshot and its health lease in
    /// one Redis transaction. Readers therefore see either an explicit
    /// unhealthy state or the complete new snapshot, never a mixture of
    /// fields sourced from different blocks.
    pub async fn publish_snapshot(
        &mut self,
        state: &PoolState,
        generation: u64,
        connection_epoch: u64,
    ) -> Result<()> {
        let payload = serde_json::to_string(&state.to_payload())?;
        let published: i32 = redis::Script::new(
            r#"
                if redis.call('GET', KEYS[1]) ~= ARGV[1]
                    or redis.call('GET', KEYS[2]) ~= ARGV[2] then
                    return 0
                end
                redis.call('SET', KEYS[3], ARGV[3])
                redis.call('SET', KEYS[4], '1', 'EX', ARGV[4])
                return 1
            "#,
        )
        .key(&self.generation_key)
        .key(&self.active_connection_epoch_key)
        .key(&self.snapshot_key)
        .key(&self.synchronized_key)
        .arg(generation.to_string())
        .arg(connection_epoch.to_string())
        .arg(payload)
        .arg(SYNCHRONIZED_TTL_SECS)
        .invoke_async(&mut self.conn)
        .await?;
        if published != 1 {
            return Err(eyre::eyre!(
                "snapshot refresh was invalidated before publication"
            ));
        }
        Ok(())
    }

    /// Read health and payload with one Redis command. `None` is deliberately
    /// used for every unhealthy/missing state so the quote path cannot fall
    /// back to stale or partially populated legacy keys.
    pub async fn snapshot(&mut self) -> Result<Option<PoolState>> {
        let (synchronized, payload): (Option<String>, Option<String>) = self
            .conn
            .mget((&self.synchronized_key, &self.snapshot_key))
            .await?;
        if synchronized.as_deref() != Some("1") {
            return Ok(None);
        }
        let Some(payload) = payload else {
            return Ok(None);
        };
        let payload: CanonicalSnapshotPayload =
            serde_json::from_str(&payload).context("invalid canonical snapshot JSON")?;
        Ok(Some(PoolState::from_payload(&payload)?))
    }
}
