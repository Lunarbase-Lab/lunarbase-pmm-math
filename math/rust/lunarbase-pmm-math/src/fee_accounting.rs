//! Conservative preflight for the fee-accounting assumptions of the finite
//! order-book price/reserve/punishment model. This does not predict token calls
//! or replace an on-chain `amountOutMinimum` guard.

use core::fmt;

use crate::{
    FillPolicyError, OrderBookState, U256Ext, ValidatedOrderBookConfig, MAX_U112,
    MAX_VALIDATION_TRANSITIONS, Q96, U256,
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
    /// Positive partner share without an operator leaves part of the fee uncredited.
    MissingPartnerOperator,
    /// A supplied bucket is outside its on-chain uint112 width.
    InvalidBucketWidth(FeeBucket),
    /// Conservative additional fees do not fit the named uint112 bucket.
    CapacityExceeded(FeeBucket),
    /// The anchor-valued cumulative input does not fit uint256.
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
            Self::BoundOverflow => f.write_str("anchor-valued signed input exceeds uint256"),
        }
    }
}

impl std::error::Error for FeeAccountingError {}

/// Check coherent fee configuration and conservative uint112 bucket headroom.
///
/// Treasury remains tradable, so cumulative fees can exceed initial output
/// reserves. Bound X fees by the anchor-valued signed Y-input cap and Y fees
/// by the signed X-input cap, using Solidity's nested floor conversions. Each
/// enabled bucket must have room for this deliberately conservative full-fee
/// bound. Disabled directions and zero shares add zero; every supplied bucket
/// is width-checked. Standard tokens and unchanged external Pool state are
/// required. Stored reserves include treasury and exclude pending deposits and
/// credited partner fees; this helper cannot detect unsynced raw balances.
pub fn try_validate_fee_accounting_capacity(
    state: &OrderBookState,
    config: &ValidatedOrderBookConfig,
) -> Result<(), FeeAccountingError> {
    let accounting = &state.fee_accounting;
    if accounting.partner_fee > PARTNER_FEE_SCALE {
        return Err(FeeAccountingError::InvalidPartnerFee);
    }
    state
        .validate()
        .map_err(|error| FeeAccountingError::Policy(error.into()))?;
    if config.max_transitions == 0 || config.max_transitions > MAX_VALIDATION_TRANSITIONS {
        return Err(FeeAccountingError::Policy(FillPolicyError::InvalidBudget));
    }
    for policy in [config.x_to_y, config.y_to_x].iter().flatten() {
        policy.validate().map_err(FeeAccountingError::Policy)?;
    }
    if accounting.partner_fee != 0 && !accounting.partner_operator_present {
        return Err(FeeAccountingError::MissingPartnerOperator);
    }
    // Treasury fees can reuse the same retained assets across fills. Cumulative
    // fees therefore cannot be bounded by initial output reserves. Bound them
    // by the anchor-valued TOTAL signed input of the opposite token instead.
    let input_x = config
        .x_to_y
        .map_or(U256::ZERO, |policy| policy.total_input);
    let input_y = config
        .y_to_x
        .map_or(U256::ZERO, |policy| policy.total_input);
    let bound_y = crate::try_x_value_in_y(input_x, state.pool.sqrt_price_x96)
        .map_err(|_| FeeAccountingError::BoundOverflow)?;
    let bound_x = if state.pool.sqrt_price_x96.is_zero() {
        U256::ZERO
    } else {
        U256::checked_mul_div(input_y, Q96, state.pool.sqrt_price_x96)
            .and_then(|first| U256::checked_mul_div(first, Q96, state.pool.sqrt_price_x96))
            .ok_or(FeeAccountingError::BoundOverflow)?
    };
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
                fee_accounting: crate::FeeAccountingState::default(),
                pool: PoolParams {
                    sqrt_price_x96: Q96,
                    fee_ask_x24: 1,
                    fee_bid_x24: 1,
                    reserve_x: 10,
                    reserve_y: 20,
                    max_punishment_x24: 0,
                    max_decay_multiplier_x24: 0,
                    current_block: 100,
                    latest_update_block: 100,
                    block_delay: 3,
                },
                fee_multiplier: U256::from(1),
                max_execution_block: 101,
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
            try_validate_fee_accounting_capacity(
                &OrderBookState {
                    fee_accounting: accounting,
                    ..state
                },
                &config
            ),
            Ok(())
        );
        accounting.partner_fee = 1;
        assert_eq!(
            try_validate_fee_accounting_capacity(
                &OrderBookState {
                    fee_accounting: accounting,
                    ..state
                },
                &config
            ),
            Err(FeeAccountingError::MissingPartnerOperator)
        );
        accounting.partner_operator_present = true;
        assert_eq!(
            try_validate_fee_accounting_capacity(
                &OrderBookState {
                    fee_accounting: accounting,
                    ..state
                },
                &config
            ),
            Ok(())
        );
        accounting.partner_fee = PARTNER_FEE_SCALE + 1;
        assert_eq!(
            try_validate_fee_accounting_capacity(
                &OrderBookState {
                    fee_accounting: accounting,
                    ..state
                },
                &config
            ),
            Err(FeeAccountingError::InvalidPartnerFee)
        );
    }

    #[test]
    fn bounds_opposite_token_input_and_checks_every_relevant_bucket() {
        let (state, config) = inputs();
        // At unit price: X fees are bounded by total Y input (6), Y fees by total X input (4).
        let exact = FeeAccountingState {
            partner_fee: PARTNER_FEE_SCALE / 2,
            partner_operator_present: true,
            treasury_x: MAX_U112 - 6,
            treasury_y: MAX_U112 - 4,
            partner_x: MAX_U112 - 6,
            partner_y: MAX_U112 - 4,
            router_partner_x: MAX_U112 - 6,
            router_partner_y: MAX_U112 - 4,
        };
        assert_eq!(
            try_validate_fee_accounting_capacity(
                &OrderBookState {
                    fee_accounting: exact,
                    ..state
                },
                &config
            ),
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
                try_validate_fee_accounting_capacity(
                    &OrderBookState {
                        fee_accounting: insufficient,
                        ..state
                    },
                    &config
                ),
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
            try_validate_fee_accounting_capacity(
                &OrderBookState {
                    fee_accounting: accounting,
                    ..state
                },
                &config
            ),
            Ok(())
        );
        accounting.partner_y = MAX_U112 + 1;
        assert_eq!(
            try_validate_fee_accounting_capacity(
                &OrderBookState {
                    fee_accounting: accounting,
                    ..state
                },
                &config
            ),
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
            try_validate_fee_accounting_capacity(
                &OrderBookState {
                    fee_accounting: accounting,
                    ..state
                },
                &config
            ),
            Ok(())
        );
    }
}
