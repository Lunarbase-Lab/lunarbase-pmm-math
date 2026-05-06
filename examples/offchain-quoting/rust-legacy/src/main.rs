#![allow(missing_docs, unreachable_pub)]

mod abi;
mod cache;
mod config;
mod handlers;
mod pool_state;
mod quoter;
mod seed;
mod ws;

use eyre::Result;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use tracing_subscriber::{
    fmt::{self, time::ChronoLocal},
    prelude::*,
    EnvFilter,
};

use crate::cache::Cache;
use crate::config::Config;

const EVENT_CHANNEL_CAPACITY: usize = 1024;
const CHANNEL_BACKPRESSURE_THRESHOLD: usize = 768;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let cfg = Config::from_env()?;

    info!(
        pool = %cfg.pool,
        rpc = %cfg.rpc_url,
        ws = %cfg.ws_url,
        redis = %redact_redis(&cfg.redis_url),
        "starting offchain quoter"
    );

    let mut event_cache = Cache::connect(&cfg.redis_url, cfg.pool).await?;
    seed::seed_state(&cfg.rpc_url, cfg.pool, &mut event_cache).await?;

    let (tx, mut rx) = mpsc::channel::<ws::ChainEvent>(EVENT_CHANNEL_CAPACITY);
    let ws_handle = tokio::spawn(ws::run(cfg.ws_url.clone(), cfg.pool, tx.clone()));

    let backpressure_handle = tokio::spawn(monitor_channel(tx));

    let event_loop = async move {
        while let Some(ev) = rx.recv().await {
            if let Err(e) = handlers::dispatch(ev, &mut event_cache).await {
                error!(error = %e, "handler failed");
            }
        }
    };

    tokio::select! {
        _ = event_loop => {
            warn!("event loop ended");
        }
        r = ws_handle => {
            warn!(?r, "WS task ended");
        }
        _ = backpressure_handle => {
            warn!("backpressure monitor ended");
        }
        _ = tokio::signal::ctrl_c() => {
            info!("ctrl-c received, shutting down");
        }
    }

    Ok(())
}

async fn monitor_channel(tx: mpsc::Sender<ws::ChainEvent>) {
    let cap = tx.max_capacity();
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(5));
    tick.tick().await;
    loop {
        tick.tick().await;
        let used = cap - tx.capacity();
        if used >= CHANNEL_BACKPRESSURE_THRESHOLD {
            warn!(
                used,
                cap, "event channel high watermark; consumer may be lagging"
            );
        }
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,offchain_quoting_example_rust=debug"));
    let timer = ChronoLocal::new("%Y-%m-%dT%H:%M:%S%.3f%:z".to_owned());
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(false).with_timer(timer))
        .init();
}

fn redact_redis(url: &str) -> String {
    if let Some((scheme, rest)) = url.split_once("://") {
        if let Some((auth, host)) = rest.split_once('@') {
            if auth.contains(':') {
                return format!("{scheme}://***@{host}");
            }
        }
    }
    url.to_owned()
}
