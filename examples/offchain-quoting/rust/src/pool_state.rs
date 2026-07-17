use alloy::primitives::aliases::U160;
use lunarbase_pmm_math::{PoolParams, U256};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default)]
pub struct PoolState {
    /// Operator-published anchor sqrt-price in Q64.96. The contract exposes a
    /// uint160, while the current math crate accepts the supported u128 range.
    /// Swaps do not mutate this value; their following `Sync` updates reserves.
    pub sqrt_price_x96: u128,
    pub fee_ask_x24: u32,
    pub fee_bid_x24: u32,
    #[allow(dead_code)]
    pub latest_update_block: u64,
    pub reserve_x: u128,
    pub reserve_y: u128,
    pub concentration_k: u32,
    #[allow(dead_code)]
    pub block_delay: u64,
    #[allow(dead_code)]
    pub paused: bool,
    /// Effective multiplier for the exact execution caller this quoter serves.
    ///
    /// This is caller-specific on-chain state: Pool.quote* checks msg.sender.
    /// Keep one cache namespace per execution adapter/router that can call the
    /// Pool directly. Do not infer this value from SwapExecuted.recipient.
    pub fee_multiplier: U256,
    #[allow(dead_code)]
    pub caller_whitelisted: bool,
    #[allow(dead_code)]
    pub blacklist_fee_multiplier: U256,
}

impl PoolState {
    pub fn to_params(&self) -> PoolParams {
        PoolParams {
            sqrt_price_x96: self.sqrt_price_x96,
            fee_ask_x24: self.fee_ask_x24,
            fee_bid_x24: self.fee_bid_x24,
            reserve_x: self.reserve_x,
            reserve_y: self.reserve_y,
            concentration_k: self.concentration_k,
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

pub fn parse_decimal_u128(s: &str) -> Option<u128> {
    s.trim().parse::<u128>().ok()
}

pub fn parse_decimal_u256(s: &str) -> Option<U256> {
    U256::from_str_radix(s.trim(), 10).ok()
}

/// Convert the contract's uint160 Q96 value without truncation. Returning
/// `None` is fail-closed: the math crate cannot quote values outside u128.
pub fn u160_to_u128_checked(value: U160) -> Option<u128> {
    let limbs = value.as_limbs();
    if limbs[2] != 0 {
        return None;
    }
    Some(((limbs[1] as u128) << 64) | limbs[0] as u128)
}
