#![allow(missing_docs, unreachable_pub)]

mod abi;
mod cache;
mod config;
mod handlers;
mod pool_state;
mod quoter;
mod seed;
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

use crate::cache::Cache;
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
        rpc = %cfg.rpc_url,
        ws = %cfg.ws_url,
        redis = %redact_redis(&cfg.redis_url),
        "starting offchain quoter"
    );
    if cfg.quote_caller == Address::ZERO {
        warn!(
            "QUOTE_CALLER_ADDRESS is address(0); this reproduces bare eth_call defaults, not a production router path"
        );
    }
    if cfg.ws_url.contains("replace-with") {
        warn!(
            "FLASH_WS is still a placeholder; seed may work, but live event logs require a real WebSocket endpoint"
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

    info!(
        timeout_secs = cfg.seed_timeout.as_secs(),
        "seeding cache from RPC"
    );
    tokio::time::timeout(
        cfg.seed_timeout,
        seed::seed_state(&cfg.rpc_url, cfg.pool, cfg.quote_caller, &mut event_cache),
    )
    .await
    .context("timed out while seeding pool state from RPC")??;
    info!("seed complete; starting live subscriptions and offline quote loop");

    let quote_cache = connect_cache(
        &cfg.redis_url,
        cfg.pool,
        cfg.quote_caller,
        cfg.redis_connect_timeout,
    )
    .await?;

    let (tx, mut rx) = mpsc::channel::<ws::ChainEvent>(EVENT_CHANNEL_CAPACITY);
    let ws_handle = tokio::spawn(ws::run(cfg.ws_url.clone(), cfg.pool, tx.clone()));

    let backpressure_handle = tokio::spawn(monitor_channel(tx));
    let quote_handle = tokio::spawn(run_demo_quotes(
        quote_cache,
        cfg.demo_quote_amount_in,
        cfg.demo_quote_x_to_y,
        cfg.demo_quote_interval,
    ));

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
    tokio::time::timeout(timeout, Cache::connect(redis_url, pool, quote_caller))
        .await
        .context(
            "timed out while connecting to Redis; check that Redis is listening on REDIS_URL",
        )?
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
        // This is intentionally Redis-only after startup. In production this
        // function is the body of your HTTP/gRPC quote handler: read one cached
        // snapshot, check freshness, compute with lunarbase-pmm-math, return.
        match quoter::quote_exact_in(&mut cache, amount_in, x_to_y).await {
            Ok(q) => {
                info!(
                    direction = if x_to_y { "X->Y" } else { "Y->X" },
                    amount_in = %amount_in,
                    amount_out = %q.amount_out,
                    fee = %q.fee,
                    fee_multiplier = %q.fee_multiplier,
                    caller_whitelisted = q.caller_whitelisted,
                    sqrt_price_next = q.sqrt_price_next,
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
