use std::env;

use alloy::primitives::Address;
use eyre::{Context, Result};

const DEFAULT_POOL: &str = "0x0000eFC4ec03a7c47D3a38A9Be7Ff1d52dD01b99";
const DEFAULT_RPC_URL: &str = "http://65.21.82.28:8545";
const DEFAULT_WS_URL: &str = "ws://65.21.82.28:8546";
const DEFAULT_REDIS_URL: &str = "redis://127.0.0.1:6379";

pub struct Config {
    pub pool: Address,
    pub rpc_url: String,
    pub ws_url: String,
    pub redis_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let _ = dotenvy::dotenv();

        let pool: Address = env::var("POOL_ADDRESS")
            .unwrap_or_else(|_| DEFAULT_POOL.to_owned())
            .parse()
            .context("POOL_ADDRESS is not a valid address")?;

        let rpc_url = env::var("RPC_URL").unwrap_or_else(|_| DEFAULT_RPC_URL.to_owned());
        let ws_url = env::var("FLASH_WS").unwrap_or_else(|_| DEFAULT_WS_URL.to_owned());
        let redis_url = env::var("REDIS_URL").unwrap_or_else(|_| DEFAULT_REDIS_URL.to_owned());

        Ok(Self {
            pool,
            rpc_url,
            ws_url,
            redis_url,
        })
    }
}
