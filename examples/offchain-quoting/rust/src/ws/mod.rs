use std::time::Duration;

use eyre::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info};

use crate::canonical_cache::Cache;

pub mod subscribe;
pub mod types;

pub use types::{ActiveConnectionEpoch, ChainEvent, ConnectionEvent};

const PING_INTERVAL: Duration = Duration::from_secs(15);
const RECONNECT_BACKOFF_MIN: Duration = Duration::from_millis(500);
const RECONNECT_BACKOFF_MAX: Duration = Duration::from_secs(15);

pub async fn run(
    ws_url: String,
    sender: mpsc::Sender<ConnectionEvent>,
    mut disconnect_cache: Cache,
    active_epoch: ActiveConnectionEpoch,
) -> Result<()> {
    let mut backoff = RECONNECT_BACKOFF_MIN;
    loop {
        let epoch = match disconnect_cache.begin_connection_epoch().await {
            Ok(epoch) => epoch,
            Err(_) => {
                error!(?backoff, "failed to allocate a connection epoch; retrying");
                sleep(backoff).await;
                backoff = (backoff * 2).min(RECONNECT_BACKOFF_MAX);
                continue;
            }
        };
        active_epoch.activate(epoch);

        match connect_loop(
            &ws_url,
            &sender,
            epoch,
            &mut disconnect_cache,
            &active_epoch,
        )
        .await
        {
            Ok(()) => {
                active_epoch.invalidate(epoch);
                disconnect_cache
                    .disconnect_connection_epoch(epoch)
                    .await
                    .map_err(|_| {
                        eyre::eyre!("failed to invalidate quote cache after WS shutdown")
                    })?;
                info!("WS loop exited cleanly");
                return Ok(());
            }
            Err(e) => {
                error!(error = %e, ?backoff, "WS connection failed; reconnecting");
                // Invalidate the in-process gate before awaiting Redis. The
                // Redis script then retires this exact epoch and advances the
                // snapshot generation atomically. Together these prevent a
                // queued or in-flight old Head from restoring health.
                active_epoch.invalidate(epoch);
                disconnect_cache
                    .disconnect_connection_epoch(epoch)
                    .await
                    .map_err(|_| {
                        eyre::eyre!("failed to invalidate quote cache after WS disconnect")
                    })?;
                sender
                    .send(ConnectionEvent::new(epoch, ChainEvent::Disconnected))
                    .await
                    .map_err(|_| eyre::eyre!("event channel closed"))?;
                sleep(backoff).await;
                backoff = (backoff * 2).min(RECONNECT_BACKOFF_MAX);
            }
        }
    }
}

async fn connect_loop(
    ws_url: &str,
    sender: &mpsc::Sender<ConnectionEvent>,
    epoch: u64,
    cache: &mut Cache,
    active_epoch: &ActiveConnectionEpoch,
) -> Result<()> {
    info!("opening confirmed-head WS");
    let (mut ws_stream, _) = connect_async(ws_url)
        .await
        .map_err(|_| eyre::eyre!("WS handshake failed"))?;

    for msg in subscribe::subscription_messages() {
        ws_stream
            .send(Message::Text(msg))
            .await
            .map_err(|_| eyre::eyre!("send subscribe failed"))?;
    }

    let mut ping_ticker = tokio::time::interval(PING_INTERVAL);
    ping_ticker.tick().await;
    let mut subscriptions = SubscriptionState::default();

    loop {
        tokio::select! {
            _ = ping_ticker.tick() => {
                if let Err(e) = ws_stream.send(Message::Ping(vec![])).await {
                    let _ = e;
                    return Err(eyre::eyre!("ping send failed"));
                }
            }
            msg = ws_stream.next() => {
                let Some(msg) = msg else {
                    return Err(eyre::eyre!("WS stream closed"));
                };
                let msg = msg.map_err(|_| eyre::eyre!("WS read error"))?;
                match msg {
                    Message::Text(text) => {
                        if let Some(event) = parse_text(&text, &mut subscriptions)
                            .context("invalid WS subscription message")?
                        {
                            enqueue_event(event, sender, epoch, cache, active_epoch).await?;
                        }
                    }
                    Message::Binary(_) => {}
                    Message::Ping(p) => {
                        ws_stream.send(Message::Pong(p)).await.ok();
                    }
                    Message::Pong(_) => {}
                    Message::Close(_) => {
                        return Err(eyre::eyre!("server closed WS"));
                    }
                    Message::Frame(_) => {}
                }
            }
        }
    }
}

#[derive(Default)]
struct SubscriptionState {
    head_id: Option<String>,
    ready: bool,
}

impl SubscriptionState {
    fn acknowledge(&mut self, request_id: &str, subscription_id: &str) -> bool {
        if request_id != subscribe::NEW_HEADS_REQUEST_ID {
            return false;
        }
        self.head_id = Some(subscription_id.to_owned());

        if !self.ready {
            self.ready = true;
            true
        } else {
            false
        }
    }

    fn route(&self, subscription_id: &str, result: &Value) -> Result<Option<ChainEvent>> {
        if !self.ready {
            return Ok(None);
        }
        if self.head_id.as_deref() == Some(subscription_id) {
            return types::head(result).map(Some).ok_or_else(|| {
                eyre::eyre!("newHeads notification has no valid block number and hash")
            });
        }
        Ok(None)
    }
}

fn parse_text(text: &str, subscriptions: &mut SubscriptionState) -> Result<Option<ChainEvent>> {
    let v: Value = serde_json::from_str(text)?;

    if let Some(id) = v.get("id").and_then(Value::as_str) {
        // Only the newHeads request is state-critical. Unknown/legacy request
        // responses (including an unsupported flashblocks error) cannot make
        // the canonical confirmed-head flow unhealthy.
        if id != subscribe::NEW_HEADS_REQUEST_ID {
            debug!(request_id = id, "ignoring unrelated subscription response");
            return Ok(None);
        }
        if let Some(err) = v.get("error") {
            return Err(eyre::eyre!("subscribe error: {err}"));
        }
        if v.get("method").is_none() {
            let subscription_id = v
                .get("result")
                .and_then(Value::as_str)
                .ok_or_else(|| eyre::eyre!("subscription acknowledgement has no string id"))?;
            debug!(request_id = id, %subscription_id, "subscribed");
            if subscriptions.acknowledge(id, subscription_id) {
                return Ok(Some(ChainEvent::Connected));
            }
            return Ok(None);
        }
    }

    let Some(method) = v.get("method").and_then(|m| m.as_str()) else {
        return Ok(None);
    };
    if method != "eth_subscription" {
        return Ok(None);
    }

    let Some(params) = v.get("params") else {
        return Ok(None);
    };
    let Some(result) = params.get("result") else {
        return Ok(None);
    };
    let sub_id = params
        .get("subscription")
        .and_then(|s| s.as_str())
        .unwrap_or("");

    subscriptions.route(sub_id, result)
}

async fn enqueue_event(
    event: ChainEvent,
    sender: &mpsc::Sender<ConnectionEvent>,
    epoch: u64,
    cache: &mut Cache,
    active_epoch: &ActiveConnectionEpoch,
) -> Result<()> {
    let connection_event = match event {
        ChainEvent::Head { number, hash } => {
            let generation = cache
                .begin_snapshot_refresh(epoch)
                .await
                .map_err(|_| eyre::eyre!("failed to invalidate cache for a confirmed head"))?
                .ok_or_else(|| eyre::eyre!("connection epoch retired before Head enqueue"))?;
            if !active_epoch.observe_head(epoch, generation) {
                return Err(eyre::eyre!(
                    "connection epoch retired while enqueueing a confirmed head"
                ));
            }
            ConnectionEvent::head(epoch, generation, number, hash)
        }
        event => ConnectionEvent::new(epoch, event),
    };
    sender
        .send(connection_event)
        .await
        .map_err(|_| eyre::eyre!("event channel closed"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_ack_alone_enables_connected_and_ignores_unsupported_flashblocks() {
        let mut subscriptions = SubscriptionState::default();

        let unrelated = parse_text(
            r#"{"jsonrpc":"2.0","id":"newFlashblocks","error":{"code":-32601,"message":"unsupported"}}"#,
            &mut subscriptions,
        )
        .unwrap();
        assert!(unrelated.is_none());

        let connected = parse_text(
            r#"{"jsonrpc":"2.0","id":"newHeads","result":"head-id"}"#,
            &mut subscriptions,
        )
        .unwrap();
        assert!(matches!(connected, Some(ChainEvent::Connected)));

        let head = parse_text(
            r#"{"jsonrpc":"2.0","method":"eth_subscription","params":{"subscription":"head-id","result":{"number":"0x2a","hash":"0x1111111111111111111111111111111111111111111111111111111111111111"}}}"#,
            &mut subscriptions,
        )
        .unwrap();
        assert!(matches!(
            head,
            Some(ChainEvent::Head { number: 42, hash })
                if hash == alloy::primitives::B256::repeat_byte(0x11)
        ));
    }

    #[test]
    fn malformed_or_incomplete_head_is_a_connection_error() {
        let mut subscriptions = SubscriptionState::default();
        subscriptions.acknowledge(subscribe::NEW_HEADS_REQUEST_ID, "head-id");

        for message in [
            r#"{"jsonrpc":"2.0","method":"eth_subscription","params":{"subscription":"head-id","result":{"number":"0x2a"}}}"#,
            r#"{"jsonrpc":"2.0","method":"eth_subscription","params":{"subscription":"head-id","result":{"number":"0x2a","hash":"nope"}}}"#,
        ] {
            assert!(parse_text(message, &mut subscriptions).is_err());
        }
    }
}
