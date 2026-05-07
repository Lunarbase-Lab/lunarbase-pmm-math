use lunarbase_pmm_math::{PoolParams, U256};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default)]
pub struct PoolState {
    pub sqrt_price_x48: u128,
    pub anchor_sqrt_price_x48: u128,
    pub fee_ask_x24: u32,
    pub fee_bid_x24: u32,
    #[allow(dead_code)]
    pub latest_update_block: u64,
    pub reserve_x: u128,
    pub reserve_y: u128,
    pub concentration_k_q12: u32,
    #[allow(dead_code)]
    pub block_delay: u64,
    #[allow(dead_code)]
    pub paused: bool,
}

impl PoolState {
    pub fn to_params(&self) -> PoolParams {
        PoolParams {
            sqrt_price_x48: self.sqrt_price_x48,
            anchor_sqrt_price_x48: self.anchor_sqrt_price_x48,
            fee_ask_x24: self.fee_ask_x24,
            fee_bid_x24: self.fee_bid_x24,
            reserve_x: self.reserve_x,
            reserve_y: self.reserve_y,
            concentration_k_q12: self.concentration_k_q12,
        }
    }

    #[allow(dead_code)]
    pub fn is_fresh(&self, head_block: u64) -> bool {
        head_block < self.latest_update_block.saturating_add(self.block_delay)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ReservesPayload(pub String, pub String);

impl ReservesPayload {
    pub fn from_pair(x: u128, y: u128) -> Self {
        Self(x.to_string(), y.to_string())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UpdatesPayload {
    pub block: u64,
    #[serde(rename = "anchorPrice")]
    pub anchor_price: String,
    #[serde(rename = "feeAskX24")]
    pub fee_ask_x24: u32,
    #[serde(rename = "feeBidX24")]
    pub fee_bid_x24: u32,
}

pub fn u256_to_u128_saturating(v: U256) -> u128 {
    if v.bit_len() > 128 {
        u128::MAX
    } else {
        let limbs = v.as_limbs();
        ((limbs[1] as u128) << 64) | (limbs[0] as u128)
    }
}

pub fn parse_decimal_u128(s: &str) -> Option<u128> {
    s.trim().parse::<u128>().ok()
}
