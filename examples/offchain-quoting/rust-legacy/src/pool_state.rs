use alloy::primitives::aliases::U160;
use lunarbase_pmm_math::{PoolParams, U256};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default)]
pub struct PoolState {
    /// Legacy pool publishes the sqrt-price in Q64.96 (uint160); the current
    /// math layer consumes the same encoding.
    pub sqrt_price_x96: U256,
    /// Legacy build still carries a single Q48 fee that we approximately
    /// re-encode as a Q24 fee for the new asymmetric API by truncating the
    /// upper 24 bits. Real deployments must migrate to per-direction fees.
    pub fee_q48: u64,
    #[allow(dead_code)]
    pub latest_update_block: u64,
    pub reserve_x: u128,
    pub reserve_y: u128,
    /// Legacy contract publishes a plain `uint32` concentration K; we shift
    /// into Q20.12 at decode time to match the math API.
    pub concentration_k: u32,
    #[allow(dead_code)]
    pub block_delay: u64,
    #[allow(dead_code)]
    pub paused: bool,
}

impl PoolState {
    pub fn to_params(&self) -> PoolParams {
        let fee_x24 = (self.fee_q48 >> 24) as u32;
        PoolParams {
            sqrt_price_x96: self.sqrt_price_x96,
            fee_ask_x24: fee_x24,
            fee_bid_x24: fee_x24,
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
    #[serde(rename = "anchorPX96")]
    pub anchor_px96: String,
    pub fee: String,
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

pub fn parse_decimal_u256(s: &str) -> Option<U256> {
    U256::from_str_radix(s.trim(), 10).ok()
}

/// Converts the legacy contract's `pX96` (alloy `U160`) into the `U256`
/// representation expected by `lunarbase_pmm_math::PoolParams`. The math
/// layer now stores sqrt-price in Q64.96 too, so this is just a width
/// widening.
pub fn px96_to_u256(p_x96: U160) -> U256 {
    let limbs = p_x96.as_limbs();
    let mut out_limbs = [0u64; 4];
    out_limbs[0] = limbs[0];
    out_limbs[1] = limbs[1];
    out_limbs[2] = limbs[2];
    U256::from_limbs(out_limbs)
}
