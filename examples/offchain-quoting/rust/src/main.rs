#![allow(missing_docs, unreachable_pub)]

mod abi;
mod canonical_cache;
mod config;
mod pool_state;
mod quoter;
mod seed;
mod snapshot_handler;
mod ws;

use alloy::primitives::Address;
use eyre::{Context, Result};
use lunarbase_pmm_math::U256;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use tracing_subscriber::{
    fmt::{self, time::ChronoLocal},
    prelude::*,
    EnvFilter,
};

use crate::canonical_cache::Cache;
use crate::config::Config;

const EVENT_CHANNEL_CAPACITY: usize = 1024;
const CHANNEL_BACKPRESSURE_THRESHOLD: usize = 768;

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = Config::from_env()?;
    init_tracing();

    info!(
        pool = %cfg.pool,
        quote_caller = %cfg.quote_caller,
        rpc = %redact_endpoint(&cfg.rpc_url),
        ws = %redact_endpoint(&cfg.ws_url),
        redis = %redact_endpoint(&cfg.redis_url),
        "starting offchain quoter"
    );
    if cfg.quote_caller == Address::ZERO {
        warn!(
            "QUOTE_CALLER_ADDRESS is address(0); this reproduces bare eth_call defaults, not a production router path"
        );
    }
    if cfg.ws_url.contains("replace-with") {
        warn!(
            "FLASH_WS is still a placeholder; quotes remain disabled until the confirmed-head subscription connects"
        );
    }

    info!(
        timeout_secs = cfg.redis_connect_timeout.as_secs(),
        "connecting Redis cache"
    );
    let mut event_cache = connect_cache(
        &cfg.redis_url,
        cfg.pool,
        cfg.quote_caller,
        cfg.redis_connect_timeout,
    )
    .await?;
    info!("Redis cache connected");
    let _ = event_cache
        .mark_unsynchronized()
        .await
        .map_err(|_| eyre::eyre!("failed to mark quote cache unhealthy"))?;
    info!("cache marked unhealthy until subscriptions are acknowledged and a pinned snapshot succeeds");

    let quote_cache = connect_cache(
        &cfg.redis_url,
        cfg.pool,
        cfg.quote_caller,
        cfg.redis_connect_timeout,
    )
    .await?;
    let disconnect_cache = connect_cache(
        &cfg.redis_url,
        cfg.pool,
        cfg.quote_caller,
        cfg.redis_connect_timeout,
    )
    .await?;

    let active_epoch = ws::ActiveConnectionEpoch::default();
    let (tx, mut rx) = mpsc::channel::<ws::ConnectionEvent>(EVENT_CHANNEL_CAPACITY);
    // The WebSocket task is started before the first RPC snapshot. It emits
    // Connected only after the confirmed-head subscription is acknowledged.
    let ws_handle = tokio::spawn(ws::run(
        cfg.ws_url.clone(),
        tx.clone(),
        disconnect_cache,
        active_epoch.clone(),
    ));

    let backpressure_handle = tokio::spawn(monitor_channel(tx));
    let quote_handle = tokio::spawn(run_demo_quotes(
        quote_cache,
        cfg.demo_quote_amount_in,
        cfg.demo_quote_x_to_y,
        cfg.demo_quote_interval,
    ));

    let rpc_url = cfg.rpc_url.clone();
    let pool = cfg.pool;
    let quote_caller = cfg.quote_caller;
    let snapshot_timeout = cfg.snapshot_timeout;
    let event_loop = async move {
        while let Some(ev) = rx.recv().await {
            if let Err(e) = snapshot_handler::dispatch(
                ev,
                &mut event_cache,
                &rpc_url,
                pool,
                quote_caller,
                snapshot_timeout,
                &active_epoch,
            )
            .await
            {
                error!(error = %e, "canonical snapshot handler failed; quotes remain unavailable");
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
        r = quote_handle => {
            warn!(?r, "quote demo task ended");
        }
        _ = tokio::signal::ctrl_c() => {
            info!("ctrl-c received, shutting down");
        }
    }

    Ok(())
}

async fn connect_cache(
    redis_url: &str,
    pool: Address,
    quote_caller: Address,
    timeout: Duration,
) -> Result<Cache> {
    let result = tokio::time::timeout(timeout, Cache::connect(redis_url, pool, quote_caller))
        .await
        .context(
            "timed out while connecting to Redis; check that Redis is listening on REDIS_URL",
        )?;
    result.map_err(|_| eyre::eyre!("failed to connect to Redis"))
}

async fn run_demo_quotes(mut cache: Cache, amount_in: U256, x_to_y: bool, interval: Duration) {
    let interval = if interval.is_zero() {
        Duration::from_secs(1)
    } else {
        interval
    };
    let mut tick = tokio::time::interval(interval);
    tick.tick().await;

    loop {
        tick.tick().await;
        // This is intentionally Redis-only. In production this
        // function is the body of your HTTP/gRPC quote handler: read one cached
        // snapshot, check freshness, compute with lunarbase-pmm-math, return.
        match quoter::quote_exact_in(&mut cache, amount_in, x_to_y).await {
            Ok(q) => {
                info!(
                    direction = if x_to_y { "X->Y" } else { "Y->X" },
                    amount_in = %amount_in,
                    amount_out = %q.amount_out,
                    fee = %q.fee,
                    effective_fee_x24 = q.effective_fee_x24,
                    fee_multiplier = %q.fee_multiplier,
                    caller_whitelisted = q.caller_whitelisted,
                    sqrt_price_next = %q.sqrt_price_next,
                    head_block = q.head_block,
                    latest_update_block = q.latest_update_block,
                    block_age = q.block_age,
                    "offline quote"
                );
            }
            Err(e) => {
                warn!(error = %e, "offline quote unavailable");
            }
        }
    }
}

async fn monitor_channel(tx: mpsc::Sender<ws::ConnectionEvent>) {
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

fn redact_endpoint(url: &str) -> String {
    url.split_once("://").map_or_else(
        || "<redacted>".to_owned(),
        |(scheme, _)| format!("{scheme}://<redacted>"),
    )
}
