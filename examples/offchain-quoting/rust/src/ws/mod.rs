use std::time::Duration;

use alloy::primitives::Address;
use eyre::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, warn};

pub mod subscribe;
pub mod types;

pub use types::ChainEvent;

const PING_INTERVAL: Duration = Duration::from_secs(15);
const RECONNECT_BACKOFF_MIN: Duration = Duration::from_millis(500);
const RECONNECT_BACKOFF_MAX: Duration = Duration::from_secs(15);

pub async fn run(ws_url: String, pool: Address, sender: mpsc::Sender<ChainEvent>) -> Result<()> {
    let mut backoff = RECONNECT_BACKOFF_MIN;
    loop {
        match connect_loop(&ws_url, pool, &sender).await {
            Ok(()) => {
                info!("WS loop exited cleanly");
                return Ok(());
            }
            Err(e) => {
                error!(error = %e, ?backoff, "WS connection failed; reconnecting");
                sleep(backoff).await;
                backoff = (backoff * 2).min(RECONNECT_BACKOFF_MAX);
            }
        }
    }
}

async fn connect_loop(
    ws_url: &str,
    pool: Address,
    sender: &mpsc::Sender<ChainEvent>,
) -> Result<()> {
    info!(%ws_url, "opening flashblocks WS");
    let (mut ws_stream, _) = connect_async(ws_url).await.context("WS handshake failed")?;

    for msg in subscribe::subscription_messages(pool) {
        ws_stream
            .send(Message::Text(msg))
            .await
            .context("send subscribe failed")?;
    }

    let mut ping_ticker = tokio::time::interval(PING_INTERVAL);
    ping_ticker.tick().await;

    loop {
        tokio::select! {
            _ = ping_ticker.tick() => {
                if let Err(e) = ws_stream.send(Message::Ping(vec![])).await {
                    return Err(eyre::eyre!("ping send failed: {e}"));
                }
            }
            msg = ws_stream.next() => {
                let Some(msg) = msg else {
                    return Err(eyre::eyre!("WS stream closed"));
                };
                let msg = msg.context("WS read error")?;
                match msg {
                    Message::Text(text) => {
                        if let Err(e) = handle_text(&text, sender).await {
                            warn!(error = %e, raw = %truncate(&text, 200), "failed to handle WS message");
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

async fn handle_text(text: &str, sender: &mpsc::Sender<ChainEvent>) -> Result<()> {
    let v: Value = serde_json::from_str(text)?;

    if let Some(id) = v.get("id") {
        if v.get("result").is_some() && v.get("method").is_none() {
            debug!(?id, "subscribed");
            return Ok(());
        }
        if let Some(err) = v.get("error") {
            return Err(eyre::eyre!("subscribe error: {err}"));
        }
    }

    let Some(method) = v.get("method").and_then(|m| m.as_str()) else {
        return Ok(());
    };
    if method != "eth_subscription" {
        return Ok(());
    }

    let Some(params) = v.get("params") else {
        return Ok(());
    };
    let result = params.get("result").cloned().unwrap_or(Value::Null);
    let sub_id = params
        .get("subscription")
        .and_then(|s| s.as_str())
        .unwrap_or("");

    let event = match types::route(sub_id, result) {
        Some(ev) => ev,
        None => return Ok(()),
    };

    sender
        .send(event)
        .await
        .map_err(|_| eyre::eyre!("event channel closed"))?;
    Ok(())
}

fn truncate(s: &str, n: usize) -> &str {
    if s.len() > n {
        &s[..n]
    } else {
        s
    }
}
