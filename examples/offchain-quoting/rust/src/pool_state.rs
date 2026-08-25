use alloy::primitives::{aliases::U160, B256};
use eyre::{ContextCompat, Result};
use lunarbase_pmm_math::{PoolParams, U256};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PoolState {
    /// Confirmed WS head number used for every field in this snapshot.
    pub snapshot_block: u64,
    /// Confirmed WS head hash used for every field through EIP-1898 with
    /// `requireCanonical=true`.
    pub snapshot_block_hash: B256,
    /// Operator-published anchor sqrt-price in Q64.96. The contract exposes a
    /// uint160 and the math crate validates that full domain without truncation.
    /// Swaps do not mutate this value; their following `Sync` updates reserves.
    pub sqrt_price_x96: U256,
    pub fee_ask_x24: u32,
    pub fee_bid_x24: u32,
    #[allow(dead_code)]
    pub latest_update_block: u64,
    pub reserve_x: u128,
    pub reserve_y: u128,
    pub max_punishment_x24: u32,
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
            max_punishment_x24: self.max_punishment_x24,
        }
    }

    pub fn is_fresh(&self) -> bool {
        self.snapshot_block < self.latest_update_block.saturating_add(self.block_delay)
    }

    pub fn block_age(&self) -> u64 {
        self.snapshot_block.saturating_sub(self.latest_update_block)
    }

    pub fn to_payload(&self) -> CanonicalSnapshotPayload {
        CanonicalSnapshotPayload {
            snapshot_block: self.snapshot_block,
            snapshot_block_hash: self.snapshot_block_hash.to_string(),
            sqrt_price_x96: self.sqrt_price_x96.to_string(),
            fee_ask_x24: self.fee_ask_x24,
            fee_bid_x24: self.fee_bid_x24,
            latest_update_block: self.latest_update_block,
            reserve_x: self.reserve_x.to_string(),
            reserve_y: self.reserve_y.to_string(),
            max_punishment_x24: self.max_punishment_x24,
            block_delay: self.block_delay,
            paused: self.paused,
            caller_whitelisted: self.caller_whitelisted,
            blacklist_fee_multiplier: self.blacklist_fee_multiplier.to_string(),
        }
    }

    pub fn from_payload(payload: &CanonicalSnapshotPayload) -> Result<Self> {
        let snapshot_block_hash = parse_block_hash(&payload.snapshot_block_hash)
            .context("cached snapshot block hash is not a canonical B256")?;
        let sqrt_price_x96 = parse_decimal_u256(&payload.sqrt_price_x96)
            .context("cached snapshot anchor is not a valid U256")?;
        let reserve_x = parse_decimal_u128(&payload.reserve_x)
            .context("cached snapshot reserveX is not a valid u128")?;
        let reserve_y = parse_decimal_u128(&payload.reserve_y)
            .context("cached snapshot reserveY is not a valid u128")?;
        let blacklist_fee_multiplier = parse_decimal_u256(&payload.blacklist_fee_multiplier)
            .context("cached snapshot blacklistFeeMultiplier is not a valid U256")?;
        if blacklist_fee_multiplier.is_zero() {
            return Err(eyre::eyre!(
                "cached snapshot blacklistFeeMultiplier must be non-zero"
            ));
        }

        let fee_multiplier = if payload.caller_whitelisted {
            U256::from(1u64)
        } else {
            blacklist_fee_multiplier
        };
        let state = Self {
            snapshot_block: payload.snapshot_block,
            snapshot_block_hash,
            sqrt_price_x96,
            fee_ask_x24: payload.fee_ask_x24,
            fee_bid_x24: payload.fee_bid_x24,
            latest_update_block: payload.latest_update_block,
            reserve_x,
            reserve_y,
            max_punishment_x24: payload.max_punishment_x24,
            block_delay: payload.block_delay,
            paused: payload.paused,
            fee_multiplier,
            caller_whitelisted: payload.caller_whitelisted,
            blacklist_fee_multiplier,
        };
        state
            .to_params()
            .validate()
            .map_err(|error| eyre::eyre!("invalid cached pool parameters: {error}"))?;
        Ok(state)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalSnapshotPayload {
    pub snapshot_block: u64,
    pub snapshot_block_hash: String,
    pub sqrt_price_x96: String,
    pub fee_ask_x24: u32,
    pub fee_bid_x24: u32,
    pub latest_update_block: u64,
    pub reserve_x: String,
    pub reserve_y: String,
    pub max_punishment_x24: u32,
    pub block_delay: u64,
    pub paused: bool,
    pub caller_whitelisted: bool,
    pub blacklist_fee_multiplier: String,
}

pub fn parse_decimal_u128(s: &str) -> Option<u128> {
    s.trim().parse::<u128>().ok()
}

pub fn parse_decimal_u256(s: &str) -> Option<U256> {
    U256::from_str_radix(s.trim(), 10).ok()
}

pub fn parse_block_hash(s: &str) -> Option<B256> {
    if !s.starts_with("0x") || s.len() != 66 {
        return None;
    }
    s.parse().ok()
}

/// Convert the contract's uint160 Q96 value into the math crate's U256 carrier
/// without truncating any of the Solidity domain.
pub fn u160_to_u256(value: U160) -> U256 {
    let limbs = value.as_limbs();
    U256::from_limbs([limbs[0], limbs[1], limbs[2], 0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_payload_round_trips_and_derives_the_effective_multiplier() {
        let state = PoolState {
            snapshot_block: 1_000,
            snapshot_block_hash: B256::repeat_byte(0x11),
            sqrt_price_x96: U256::from(1u64) << 96,
            fee_ask_x24: 11,
            fee_bid_x24: 12,
            latest_update_block: 990,
            reserve_x: 13,
            reserve_y: 14,
            max_punishment_x24: 15,
            block_delay: 20,
            paused: false,
            fee_multiplier: U256::from(999u64),
            caller_whitelisted: true,
            blacklist_fee_multiplier: U256::from(7u64),
        };

        let decoded = PoolState::from_payload(&state.to_payload()).unwrap();
        assert_eq!(decoded.fee_multiplier, U256::from(1u64));
        assert_eq!(decoded.snapshot_block_hash, B256::repeat_byte(0x11));
        assert!(decoded.is_fresh());
        assert_eq!(decoded.block_age(), 10);
    }

    #[test]
    fn canonical_payload_rejects_corrupt_solidity_widths() {
        let payload = CanonicalSnapshotPayload {
            snapshot_block: 1,
            snapshot_block_hash:
                "0x1111111111111111111111111111111111111111111111111111111111111111".to_owned(),
            sqrt_price_x96: (U256::from(1u64) << 160usize).to_string(),
            fee_ask_x24: 0,
            fee_bid_x24: 0,
            latest_update_block: 1,
            reserve_x: "1".to_owned(),
            reserve_y: "1".to_owned(),
            max_punishment_x24: 0,
            block_delay: 1,
            paused: false,
            caller_whitelisted: false,
            blacklist_fee_multiplier: "1".to_owned(),
        };

        assert!(PoolState::from_payload(&payload).is_err());
    }

    #[test]
    fn canonical_payload_rejects_malformed_block_hash() {
        let mut payload = PoolState {
            snapshot_block: 1,
            snapshot_block_hash: B256::repeat_byte(0x11),
            sqrt_price_x96: U256::from(1u64) << 96,
            fee_ask_x24: 0,
            fee_bid_x24: 0,
            latest_update_block: 1,
            reserve_x: 1,
            reserve_y: 1,
            max_punishment_x24: 0,
            block_delay: 1,
            paused: false,
            fee_multiplier: U256::from(1u64),
            caller_whitelisted: true,
            blacklist_fee_multiplier: U256::from(1u64),
        }
        .to_payload();
        payload.snapshot_block_hash = "0x1234".to_owned();

        assert!(PoolState::from_payload(&payload).is_err());
    }
}
