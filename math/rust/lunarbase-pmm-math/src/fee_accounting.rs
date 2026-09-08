//! Conservative preflight for the fee-accounting assumptions of the finite
//! order-book price/reserve/punishment model. This does not predict token calls
//! or replace an on-chain `amountOutMinimum` guard.

use core::fmt;

use crate::{
    FillPolicyError, OrderBookState, ValidatedOrderBookConfig, MAX_U112,
    MAX_VALIDATION_TRANSITIONS, U256,
};

/// Pool denominator for the configured partner share (100%).
pub const PARTNER_FEE_SCALE: u32 = 1_000_000;

/// Fee configuration and uint112 buckets from the same coherent Pool snapshot.
/// The per-router fields must belong to the adapter that calls the Pool.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FeeAccountingState {
    /// Partner share of charged fees, scaled by 1e6 (`FeeManager.BPS`).
    pub partner_fee: u32,
    /// Whether this adapter/router has a nonzero partner operator.
    pub partner_operator_present: bool,
    /// Global treasury fee bucket for token X.
    pub treasury_x: u128,
    /// Global treasury fee bucket for token Y.
    pub treasury_y: u128,
    /// Global partner fee bucket for token X.
    pub partner_x: u128,
    /// Global partner fee bucket for token Y.
    pub partner_y: u128,
    /// This adapter/router's cumulative partner bucket for token X.
    pub router_partner_x: u128,
    /// This adapter/router's cumulative partner bucket for token Y.
    pub router_partner_y: u128,
}

/// Specific on-chain uint112 bucket involved in a preflight failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeeBucket {
    /// Global treasury X.
    TreasuryX,
    /// Global treasury Y.
    TreasuryY,
    /// Global partner X.
    PartnerX,
    /// Global partner Y.
    PartnerY,
    /// This router's partner X.
    RouterPartnerX,
    /// This router's partner Y.
    RouterPartnerY,
}

/// Fail-closed fee-accounting or coherent-state preflight error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeeAccountingError {
    /// Invalid underlying snapshot or fill policy.
    Policy(FillPolicyError),
    /// Partner share exceeds the Pool's 1e6 denominator.
    InvalidPartnerFee,
    /// Positive partner share without operator skips credit and invalidates
    /// the model's full-gross reserve debit assumption.
    MissingPartnerOperator,
    /// A supplied bucket is outside its on-chain uint112 width.
    InvalidBucketWidth(FeeBucket),
    /// Conservative additional fees do not fit the named uint112 bucket.
    CapacityExceeded(FeeBucket),
    /// Even a uint256 cannot contain reserve plus signed same-token input.
    BoundOverflow,
}

impl fmt::Display for FeeAccountingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Policy(error) => error.fmt(f),
            Self::InvalidPartnerFee => f.write_str("partner_fee exceeds 1e6"),
            Self::MissingPartnerOperator => f.write_str(
                "positive partner_fee requires an operator so all charged fees are credited",
            ),
            Self::InvalidBucketWidth(bucket) => write!(f, "{bucket:?} fee bucket exceeds uint112"),
            Self::CapacityExceeded(bucket) => write!(
                f,
                "{bucket:?} fee bucket lacks conservative uint112 headroom"
            ),
            Self::BoundOverflow => f.write_str("reserve plus signed input overflows uint256"),
        }
    }
}

impl std::error::Error for FeeAccountingError {}

/// Check conditions needed for fully credited fees and accounting headroom.
///
/// For each token paid as output by an enabled direction, cumulative charged
/// fees are bounded by its initial active reserve plus all signed input of that
/// SAME token. Output and fees cannot consume more tokens than this budget in
/// the standard-token, fully credited model. Every relevant fee bucket must
/// have that much headroom; this deliberately overestimates the actual fee
/// share. A disabled output direction contributes zero. Existing buckets are
/// always width-checked, including when their configured share is zero.
///
/// This is conservative and may reject feasible books near bucket limits. It
/// assumes no external swaps, fee/config changes or concurrently live quote
/// generations beyond `config`. Together with the certified price model it
/// still cannot guarantee arbitrary ERC20 calls, permissions, or EVM execution.
/// Initial stored reserves must already reconcile with token balances minus
/// pending-deposit escrow and global fee buckets; this helper cannot detect an
/// unsynced donation or another raw-balance discrepancy.
pub fn try_validate_fee_accounting_capacity(
    state: &OrderBookState,
    config: &ValidatedOrderBookConfig,
    accounting: &FeeAccountingState,
) -> Result<(), FeeAccountingError> {
    state
        .validate()
        .map_err(|error| FeeAccountingError::Policy(error.into()))?;
    if config.max_transitions == 0 || config.max_transitions > MAX_VALIDATION_TRANSITIONS {
        return Err(FeeAccountingError::Policy(FillPolicyError::InvalidBudget));
    }
    for policy in [config.x_to_y, config.y_to_x].iter().flatten() {
        policy.validate().map_err(FeeAccountingError::Policy)?;
    }
    if accounting.partner_fee > PARTNER_FEE_SCALE {
        return Err(FeeAccountingError::InvalidPartnerFee);
    }
    if accounting.partner_fee != 0 && !accounting.partner_operator_present {
        return Err(FeeAccountingError::MissingPartnerOperator);
    }
    let budget = |output_enabled: bool, reserve: u128, input: Option<crate::FillPolicy>| {
        if !output_enabled {
            return Ok(U256::ZERO);
        }
        U256::from(reserve)
            .checked_add(input.map_or(U256::ZERO, |policy| policy.total_input))
            .ok_or(FeeAccountingError::BoundOverflow)
    };
    let bound_x = budget(config.y_to_x.is_some(), state.pool.reserve_x, config.x_to_y)?;
    let bound_y = budget(config.x_to_y.is_some(), state.pool.reserve_y, config.y_to_x)?;
    let treasury_enabled = accounting.partner_fee < PARTNER_FEE_SCALE;
    let partner_enabled = accounting.partner_fee > 0;
    for (bucket, current, bound, enabled) in [
        (
            FeeBucket::TreasuryX,
            accounting.treasury_x,
            bound_x,
            treasury_enabled,
        ),
        (
            FeeBucket::TreasuryY,
            accounting.treasury_y,
            bound_y,
            treasury_enabled,
        ),
        (
            FeeBucket::PartnerX,
            accounting.partner_x,
            bound_x,
            partner_enabled,
        ),
        (
            FeeBucket::PartnerY,
            accounting.partner_y,
            bound_y,
            partner_enabled,
        ),
        (
            FeeBucket::RouterPartnerX,
            accounting.router_partner_x,
            bound_x,
            partner_enabled,
        ),
        (
            FeeBucket::RouterPartnerY,
            accounting.router_partner_y,
            bound_y,
            partner_enabled,
        ),
    ] {
        if current > MAX_U112 {
            return Err(FeeAccountingError::InvalidBucketWidth(bucket));
        }
        if enabled && bound > U256::from(MAX_U112 - current) {
            return Err(FeeAccountingError::CapacityExceeded(bucket));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FillPolicy, PoolParams, Q96};

    fn inputs() -> (OrderBookState, ValidatedOrderBookConfig) {
        (
            OrderBookState {
                pool: PoolParams {
                    sqrt_price_x96: Q96,
                    fee_ask_x24: 1,
                    fee_bid_x24: 1,
                    reserve_x: 10,
                    reserve_y: 20,
                    max_punishment_x24: 0,
                },
                fee_multiplier: U256::from(1),
                snapshot_block: 100,
                max_execution_block: 101,
                latest_update_block: 100,
                block_delay: 3,
                paused: false,
            },
            ValidatedOrderBookConfig {
                x_to_y: Some(FillPolicy {
                    min_input: U256::from(1),
                    lot_input: U256::from(1),
                    max_input: U256::from(2),
                    total_input: U256::from(4),
                }),
                y_to_x: Some(FillPolicy {
                    min_input: U256::from(1),
                    lot_input: U256::from(1),
                    max_input: U256::from(3),
                    total_input: U256::from(6),
                }),
                max_transitions: 100,
            },
        )
    }

    #[test]
    fn rejects_positive_share_without_operator_and_invalid_share() {
        let (state, config) = inputs();
        let mut accounting = FeeAccountingState::default();
        assert_eq!(
            try_validate_fee_accounting_capacity(&state, &config, &accounting),
            Ok(())
        );
        accounting.partner_fee = 1;
        assert_eq!(
            try_validate_fee_accounting_capacity(&state, &config, &accounting),
            Err(FeeAccountingError::MissingPartnerOperator)
        );
        accounting.partner_operator_present = true;
        assert_eq!(
            try_validate_fee_accounting_capacity(&state, &config, &accounting),
            Ok(())
        );
        accounting.partner_fee = PARTNER_FEE_SCALE + 1;
        assert_eq!(
            try_validate_fee_accounting_capacity(&state, &config, &accounting),
            Err(FeeAccountingError::InvalidPartnerFee)
        );
    }

    #[test]
    fn includes_same_token_input_and_checks_every_relevant_bucket() {
        let (state, config) = inputs();
        // X bound=10+4; Y bound=20+6, regardless of output amounts or split.
        let exact = FeeAccountingState {
            partner_fee: PARTNER_FEE_SCALE / 2,
            partner_operator_present: true,
            treasury_x: MAX_U112 - 14,
            treasury_y: MAX_U112 - 26,
            partner_x: MAX_U112 - 14,
            partner_y: MAX_U112 - 26,
            router_partner_x: MAX_U112 - 14,
            router_partner_y: MAX_U112 - 26,
        };
        assert_eq!(
            try_validate_fee_accounting_capacity(&state, &config, &exact),
            Ok(())
        );
        for bucket in [
            FeeBucket::TreasuryX,
            FeeBucket::TreasuryY,
            FeeBucket::PartnerX,
            FeeBucket::PartnerY,
            FeeBucket::RouterPartnerX,
            FeeBucket::RouterPartnerY,
        ] {
            let mut insufficient = exact;
            match bucket {
                FeeBucket::TreasuryX => insufficient.treasury_x += 1,
                FeeBucket::TreasuryY => insufficient.treasury_y += 1,
                FeeBucket::PartnerX => insufficient.partner_x += 1,
                FeeBucket::PartnerY => insufficient.partner_y += 1,
                FeeBucket::RouterPartnerX => insufficient.router_partner_x += 1,
                FeeBucket::RouterPartnerY => insufficient.router_partner_y += 1,
            }
            assert_eq!(
                try_validate_fee_accounting_capacity(&state, &config, &insufficient),
                Err(FeeAccountingError::CapacityExceeded(bucket))
            );
        }
    }

    #[test]
    fn ignores_disabled_outputs_and_zero_shares_but_never_invalid_widths() {
        let (state, mut config) = inputs();
        config.y_to_x = None;
        let mut accounting = FeeAccountingState {
            treasury_x: MAX_U112,
            partner_x: MAX_U112,
            partner_y: MAX_U112,
            router_partner_x: MAX_U112,
            router_partner_y: MAX_U112,
            ..FeeAccountingState::default()
        };
        assert_eq!(
            try_validate_fee_accounting_capacity(&state, &config, &accounting),
            Ok(())
        );
        accounting.partner_y = MAX_U112 + 1;
        assert_eq!(
            try_validate_fee_accounting_capacity(&state, &config, &accounting),
            Err(FeeAccountingError::InvalidBucketWidth(FeeBucket::PartnerY))
        );
        accounting = FeeAccountingState {
            partner_fee: PARTNER_FEE_SCALE,
            partner_operator_present: true,
            treasury_x: MAX_U112,
            treasury_y: MAX_U112,
            ..FeeAccountingState::default()
        };
        assert_eq!(
            try_validate_fee_accounting_capacity(&state, &config, &accounting),
            Ok(())
        );
    }
}
