use std::env;
use std::path::Path;
use std::time::Duration;

use alloy::primitives::Address;
use eyre::{Context, Result};
use lunarbase_pmm_math::U256;

const DEFAULT_POOL: &str = "0x0000eFC4ec03a7c47D3a38A9Be7Ff1d52dD01b99";
const DEFAULT_QUOTE_CALLER: &str = "0x0000000000000000000000000000000000000000";
const DEFAULT_REDIS_URL: &str = "redis://127.0.0.1:6379";
const DEFAULT_QUOTE_AMOUNT_IN: &str = "1000000000000000";
const DEFAULT_QUOTE_DIRECTION: &str = "x_to_y";
const DEFAULT_QUOTE_INTERVAL_SECS: u64 = 5;
const DEFAULT_SEED_TIMEOUT_SECS: u64 = 20;
const DEFAULT_REDIS_CONNECT_TIMEOUT_SECS: u64 = 5;

pub struct Config {
    pub pool: Address,
    pub quote_caller: Address,
    pub rpc_url: String,
    pub ws_url: String,
    pub redis_url: String,
    pub demo_quote_amount_in: U256,
    pub demo_quote_x_to_y: bool,
    pub demo_quote_interval: Duration,
    pub seed_timeout: Duration,
    pub redis_connect_timeout: Duration,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        load_dotenv();

        let pool: Address = env::var("POOL_ADDRESS")
            .unwrap_or_else(|_| DEFAULT_POOL.to_owned())
            .parse()
            .context("POOL_ADDRESS is not a valid address")?;

        // Best practice: set this to the exact router/adapter/settlement
        // contract that directly calls the Pool in production. Omitting it
        // intentionally defaults to address(0), which is useful for reproducing
        // bare eth_call/cast-call behavior but is usually not executable flow.
        let quote_caller: Address = env::var("QUOTE_CALLER_ADDRESS")
            .unwrap_or_else(|_| DEFAULT_QUOTE_CALLER.to_owned())
            .parse()
            .context("QUOTE_CALLER_ADDRESS is not a valid address")?;

        let rpc_url = required_env("RPC_URL")?;
        let ws_url = required_env("FLASH_WS")?;
        let redis_url = env::var("REDIS_URL").unwrap_or_else(|_| DEFAULT_REDIS_URL.to_owned());
        let demo_quote_amount_in = parse_u256_env("QUOTE_AMOUNT_IN", DEFAULT_QUOTE_AMOUNT_IN)?;
        let demo_quote_x_to_y = parse_direction(
            &env::var("QUOTE_DIRECTION").unwrap_or_else(|_| DEFAULT_QUOTE_DIRECTION.to_owned()),
        )?;
        let demo_quote_interval = Duration::from_secs(
            env::var("QUOTE_INTERVAL_SECS")
                .ok()
                .map(|s| s.parse().context("QUOTE_INTERVAL_SECS must be a u64"))
                .transpose()?
                .unwrap_or(DEFAULT_QUOTE_INTERVAL_SECS),
        );
        let seed_timeout = Duration::from_secs(
            env::var("SEED_TIMEOUT_SECS")
                .ok()
                .map(|s| s.parse().context("SEED_TIMEOUT_SECS must be a u64"))
                .transpose()?
                .unwrap_or(DEFAULT_SEED_TIMEOUT_SECS),
        );
        let redis_connect_timeout = Duration::from_secs(
            env::var("REDIS_CONNECT_TIMEOUT_SECS")
                .ok()
                .map(|s| {
                    s.parse()
                        .context("REDIS_CONNECT_TIMEOUT_SECS must be a u64")
                })
                .transpose()?
                .unwrap_or(DEFAULT_REDIS_CONNECT_TIMEOUT_SECS),
        );

        Ok(Self {
            pool,
            quote_caller,
            rpc_url,
            ws_url,
            redis_url,
            demo_quote_amount_in,
            demo_quote_x_to_y,
            demo_quote_interval,
            seed_timeout,
            redis_connect_timeout,
        })
    }
}

fn load_dotenv() {
    let _ = dotenvy::dotenv();
    let manifest_env = Path::new(env!("CARGO_MANIFEST_DIR")).join(".env");
    let _ = dotenvy::from_path(manifest_env);
}

fn required_env(name: &str) -> Result<String> {
    let value = env::var(name).context(format!("{name} must be set"))?;
    if value.trim().is_empty() {
        return Err(eyre::eyre!("{name} must not be empty"));
    }
    Ok(value)
}

fn parse_u256_env(name: &str, default: &str) -> Result<U256> {
    let raw = env::var(name).unwrap_or_else(|_| default.to_owned());
    U256::from_str_radix(raw.trim(), 10).context(format!("{name} must be a decimal uint256"))
}

fn parse_direction(raw: &str) -> Result<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "x_to_y" | "x2y" | "x->y" | "true" | "1" => Ok(true),
        "y_to_x" | "y2x" | "y->x" | "false" | "0" => Ok(false),
        _ => Err(eyre::eyre!(
            "QUOTE_DIRECTION must be x_to_y or y_to_x, got {raw}"
        )),
    }
}
