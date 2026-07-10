use alloy::primitives::Address;
use eyre::{Context, ContextCompat, Result};
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

use crate::pool_state::{parse_decimal_u128, PoolState, ReservesPayload, UpdatesPayload};

const RESERVES_TTL: u64 = 10;
const UPD_TTL: u64 = 6;
const LOG_TTL: u64 = 10;

pub struct Cache {
    pool_tag: String,
    conn: ConnectionManager,
}

impl Cache {
    pub async fn connect(redis_url: &str, pool: Address) -> Result<Self> {
        let client = redis::Client::open(redis_url).context("invalid REDIS_URL")?;
        let conn = ConnectionManager::new(client)
            .await
            .context("failed to connect to Redis")?;
        Ok(Self {
            pool_tag: format!("{:#x}", pool),
            conn,
        })
    }

    fn k_reserves(&self) -> String {
        format!("reserves:{}", self.pool_tag)
    }
    fn k_updates(&self) -> String {
        format!("updates:{}", self.pool_tag)
    }
    fn k_concentration_k(&self) -> String {
        format!("pmm:concentrationK:{}", self.pool_tag)
    }
    fn k_block_delay(&self) -> String {
        format!("pmm:blockDelay:{}", self.pool_tag)
    }
    fn k_paused(&self) -> String {
        format!("pmm:paused:{}", self.pool_tag)
    }
    fn k_log_dedup(&self, fingerprint: &str) -> String {
        format!("log:tx:{}:{}", self.pool_tag, fingerprint)
    }
    fn k_head(&self) -> String {
        format!("head:{}", self.pool_tag)
    }

    pub async fn try_take_log(&mut self, fingerprint: &str) -> Result<bool> {
        let key = self.k_log_dedup(fingerprint);
        let res: Option<String> = redis::cmd("SET")
            .arg(&key)
            .arg("1")
            .arg("EX")
            .arg(LOG_TTL)
            .arg("NX")
            .query_async(&mut self.conn)
            .await?;
        Ok(res.is_some())
    }

    pub async fn set_reserves(&mut self, x: u128, y: u128) -> Result<()> {
        let payload = serde_json::to_string(&ReservesPayload::from_pair(x, y))?;
        let _: () = self
            .conn
            .set_ex(self.k_reserves(), payload, RESERVES_TTL)
            .await?;
        Ok(())
    }

    pub async fn set_state(
        &mut self,
        block: u64,
        anchor_price: u128,
        fee_ask_x24: u32,
        fee_bid_x24: u32,
    ) -> Result<()> {
        let payload = serde_json::to_string(&UpdatesPayload {
            block,
            anchor_price: anchor_price.to_string(),
            fee_ask_x24,
            fee_bid_x24,
        })?;
        let _: () = self.conn.set_ex(self.k_updates(), payload, UPD_TTL).await?;
        Ok(())
    }

    pub async fn set_concentration_k(&mut self, k: u32) -> Result<()> {
        let _: () = self
            .conn
            .set(self.k_concentration_k(), k.to_string())
            .await?;
        Ok(())
    }

    pub async fn set_block_delay(&mut self, d: u64) -> Result<()> {
        let _: () = self.conn.set(self.k_block_delay(), d.to_string()).await?;
        Ok(())
    }

    pub async fn set_paused(&mut self, p: bool) -> Result<()> {
        let _: () = self
            .conn
            .set(self.k_paused(), if p { "1" } else { "0" })
            .await?;
        Ok(())
    }

    pub async fn set_head_block(&mut self, n: u64) -> Result<()> {
        let _: () = self.conn.set_ex(self.k_head(), n.to_string(), 30).await?;
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn get_head_block(&mut self) -> Result<Option<u64>> {
        let v: Option<String> = self.conn.get(self.k_head()).await?;
        Ok(v.and_then(|s| s.parse().ok()))
    }

    pub async fn snapshot(&mut self) -> Result<Option<PoolState>> {
        let keys = vec![
            self.k_reserves(),
            self.k_updates(),
            self.k_concentration_k(),
            self.k_block_delay(),
            self.k_paused(),
        ];
        let raw: Vec<Option<String>> = self.conn.mget(keys).await?;
        let reserves = raw[0].as_ref();
        let updates = raw[1].as_ref();
        let concentration_k = raw[2].as_ref();
        let block_delay = raw[3].as_ref();
        let paused = raw[4].as_ref();

        let (Some(reserves), Some(updates), Some(concentration_k), Some(block_delay), Some(paused)) =
            (reserves, updates, concentration_k, block_delay, paused)
        else {
            return Ok(None);
        };

        let r: ReservesPayload = serde_json::from_str(reserves)?;
        let u: UpdatesPayload = serde_json::from_str(updates)?;

        let reserve_x = parse_decimal_u128(&r.0).context("invalid cached reserveX")?;
        let reserve_y = parse_decimal_u128(&r.1).context("invalid cached reserveY")?;
        let sqrt_price_x96 = parse_decimal_u128(&u.anchor_price)
            .context("cached anchorPrice does not fit the math crate's u128 Q96 range")?;

        let concentration_k = concentration_k
            .parse::<u32>()
            .context("invalid cached concentrationK")?;
        let block_delay = block_delay
            .parse::<u64>()
            .context("invalid cached blockDelay")?;
        let paused = match paused.as_str() {
            "0" => false,
            "1" => true,
            _ => return Err(eyre::eyre!("invalid cached paused flag")),
        };

        Ok(Some(PoolState {
            sqrt_price_x96,
            fee_ask_x24: u.fee_ask_x24,
            fee_bid_x24: u.fee_bid_x24,
            latest_update_block: u.block,
            reserve_x,
            reserve_y,
            concentration_k,
            block_delay,
            paused,
        }))
    }
}
