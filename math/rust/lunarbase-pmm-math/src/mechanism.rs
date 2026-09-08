//! Integer-exact mirror of the current Solidity `SwapLib` quote and
//! directional-punishment paths.
//!
//! Quotes are linear at the operator-published Q64.96 anchor. Each quote adds
//! the current swap's punishment to the stored directional fee before pricing
//! the output. A successful swap persists that effective fee for subsequent
//! swaps. There is no concentration curve or swap-driven price state.

use core::fmt;

use crate::uint256::{U256Ext, U256};

/// Q24 fixed-point unit (`2^24`), representing conceptual 100%.
pub const Q24: u32 = 1u32 << 24;
/// Largest value representable by Solidity `uint24`.
pub const MAX_U24: u32 = Q24 - 1;
/// Largest value representable by Solidity `uint112`.
pub const MAX_U112: u128 = (1u128 << 112) - 1;
/// On-chain maximum-punishment sentinel.
pub const MAX_PUNISHMENT_X24: u32 = MAX_U24;

/// Q64.96 fixed-point unit (`2^96`).
pub const Q96: U256 = U256::from_limbs([0, 1u64 << 32, 0, 0]);
/// Largest value representable by Solidity `uint160`.
pub const MAX_U160: U256 = U256::from_limbs([u64::MAX, u64::MAX, u32::MAX as u64, 0]);

/// Direction of an exact-input swap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    /// Token X in, token Y out; charges and increments the bid fee.
    XToY,
    /// Token Y in, token X out; charges and increments the ask fee.
    YToX,
}

/// Checked-math or Solidity-width validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MathError {
    /// `sqrt_price_x96` exceeds Solidity `uint160`.
    SqrtPriceExceedsUint160,
    /// `fee_ask_x24` exceeds Solidity `uint24`.
    FeeAskExceedsUint24,
    /// `fee_bid_x24` exceeds Solidity `uint24`.
    FeeBidExceedsUint24,
    /// `max_punishment_x24` exceeds Solidity `uint24`.
    MaxPunishmentExceedsUint24,
    /// A standalone fee argument exceeds Solidity `uint24`.
    FeeExceedsUint24,
    /// X reserve exceeds Solidity `uint112`.
    ReserveXExceedsUint112,
    /// Y reserve exceeds Solidity `uint112`.
    ReserveYExceedsUint112,
    /// A standalone punishment increment exceeds Solidity `uint24`.
    PunishmentExceedsUint24,
    /// A checked division used a zero denominator.
    DivisionByZero,
    /// A full-precision `mulDiv` quotient does not fit in `uint256`.
    MulDivOverflow,
    /// A `uint256` addition overflowed.
    ArithmeticOverflow,
}

impl fmt::Display for MathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::SqrtPriceExceedsUint160 => "sqrt_price_x96 exceeds uint160",
            Self::FeeAskExceedsUint24 => "fee_ask_x24 exceeds uint24",
            Self::FeeBidExceedsUint24 => "fee_bid_x24 exceeds uint24",
            Self::MaxPunishmentExceedsUint24 => "max_punishment_x24 exceeds uint24",
            Self::FeeExceedsUint24 => "fee exceeds uint24",
            Self::ReserveXExceedsUint112 => "reserve_x exceeds uint112",
            Self::ReserveYExceedsUint112 => "reserve_y exceeds uint112",
            Self::PunishmentExceedsUint24 => "punishment exceeds uint24",
            Self::DivisionByZero => "division by zero",
            Self::MulDivOverflow => "mulDiv result exceeds uint256",
            Self::ArithmeticOverflow => "uint256 arithmetic overflow",
        })
    }
}

impl std::error::Error for MathError {}

/// Snapshot of the on-chain fields required for quotes and punishment.
///
/// The two fee fields are the current effective fees. They already include
/// any punishment accumulated since the latest operator `upd` call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PoolParams {
    /// Operator-published anchor sqrt-price in Q64.96 (`uint160` on-chain).
    pub sqrt_price_x96: U256,
    /// Current Y→X fee in Q24 (`uint24` on-chain).
    pub fee_ask_x24: u32,
    /// Current X→Y fee in Q24 (`uint24` on-chain).
    pub fee_bid_x24: u32,
    /// Active reserve of token X (`uint112` on-chain).
    pub reserve_x: u128,
    /// Active reserve of token Y (`uint112` on-chain).
    pub reserve_y: u128,
    /// Maximum directional punishment in Q24. `uint24::MAX` is the 100%
    /// sentinel; zero disables punishment.
    pub max_punishment_x24: u32,
}

impl PoolParams {
    /// Validate every field against its Solidity storage width.
    pub fn validate(&self) -> Result<(), MathError> {
        if !self.sqrt_price_x96.fits_u160() {
            return Err(MathError::SqrtPriceExceedsUint160);
        }
        if self.fee_ask_x24 > MAX_U24 {
            return Err(MathError::FeeAskExceedsUint24);
        }
        if self.fee_bid_x24 > MAX_U24 {
            return Err(MathError::FeeBidExceedsUint24);
        }
        if self.reserve_x > MAX_U112 {
            return Err(MathError::ReserveXExceedsUint112);
        }
        if self.reserve_y > MAX_U112 {
            return Err(MathError::ReserveYExceedsUint112);
        }
        if self.max_punishment_x24 > MAX_U24 {
            return Err(MathError::MaxPunishmentExceedsUint24);
        }
        Ok(())
    }
}

/// Result of an exact-input quote.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuoteResult {
    /// Output amount after the caller-specific directional fee.
    pub amount_out: U256,
    /// Always the unchanged operator anchor.
    pub sqrt_price_next: U256,
    /// Fee charged in the output token.
    pub fee: U256,
    /// Saturating directional fee used by this quote before the caller
    /// multiplier. This is counterfactual when the surrounding swap reverts.
    pub effective_fee_x24: u32,
}

/// Attempted directional punishment transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PunishmentTransition {
    /// Increment requested by the wealth-ratio formula.
    pub desired_punishment_x24: u32,
    /// Increment actually applied after saturating against the current fee.
    pub applied_punishment_x24: u32,
    /// Ask fee after the attempted transition.
    pub fee_ask_x24: u32,
    /// Bid fee after the attempted transition.
    pub fee_bid_x24: u32,
}

/// Why a simulated Solidity swap reverted and rolled all state back.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RollbackReason {
    /// The quote returned zero output, matching `SwapImpossible`.
    SwapImpossible,
    /// Post-transfer active reserves would not fit in `uint112`.
    ReserveTransitionOverflow,
    /// A caller marked a later transfer/accounting failure that the pure math
    /// cannot predict.
    LaterRevert,
}

/// Commit status of a swap simulation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SimulationStatus {
    /// Quote, punishment, and reserve transition all committed.
    Applied,
    /// Solidity atomicity restores `pre_swap`.
    RolledBack(RollbackReason),
}

/// Full state transition for one attempted exact-input swap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SwapSimulation {
    /// Quote produced from `pre_swap` with the current punishment included.
    pub quote: QuoteResult,
    /// Punishment included in the quote and persisted only on success.
    pub punishment: PunishmentTransition,
    /// Original state, retained as the rollback snapshot.
    pub pre_swap: PoolParams,
    /// Committed state, or `pre_swap` when `status` is rolled back.
    pub post_swap: PoolParams,
    /// Whether the simulated transaction committed.
    pub status: SimulationStatus,
}

impl SwapSimulation {
    /// Return the state visible after applying Solidity transaction atomicity.
    #[must_use]
    pub const fn effective_params(&self) -> &PoolParams {
        match self.status {
            SimulationStatus::Applied => &self.post_swap,
            SimulationStatus::RolledBack(_) => &self.pre_swap,
        }
    }

    /// Mark a later external failure and restore the rollback snapshot.
    pub fn mark_rolled_back(&mut self, reason: RollbackReason) {
        self.punishment.applied_punishment_x24 = 0;
        self.punishment.fee_ask_x24 = self.pre_swap.fee_ask_x24;
        self.punishment.fee_bid_x24 = self.pre_swap.fee_bid_x24;
        self.post_swap = self.pre_swap;
        self.status = SimulationStatus::RolledBack(reason);
    }
}

/// Convert a plain decimal price into a Q64.96 sqrt-price.
///
/// This convenience adapter is lossy beyond `f64`'s 53-bit significand. It
/// panics for negative, NaN, or infinite values and saturates at `uint160::MAX`.
#[inline]
pub fn price_to_sqrt_price_x96(price: f64) -> U256 {
    assert!(
        price.is_finite() && price >= 0.0,
        "price must be finite and non-negative"
    );
    let scaled = price.sqrt() * 2f64.powi(96);
    if scaled < 1.0 {
        return U256::ZERO;
    }
    if !scaled.is_finite() || scaled >= 2f64.powi(160) {
        return MAX_U160;
    }
    f64_floor_to_u256(scaled)
}

/// Convert a Q64.96 sqrt-price back to a plain decimal price.
#[inline]
pub fn sqrt_price_x96_to_price(sqrt_price_x96: U256) -> f64 {
    let sqrt_price = u256_to_f64_lossy(sqrt_price_x96) / 2f64.powi(96);
    sqrt_price * sqrt_price
}

fn f64_floor_to_u256(value: f64) -> U256 {
    if value < 1.0 {
        return U256::ZERO;
    }
    let bits = value.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32 - 1023;
    let mantissa = (bits & ((1u64 << 52) - 1)) | (1u64 << 52);
    let integer = U256::from(mantissa);
    if exponent >= 52 {
        integer << (exponent - 52) as usize
    } else {
        integer >> (52 - exponent) as usize
    }
}

fn u256_to_f64_lossy(value: U256) -> f64 {
    if value.is_zero() {
        return 0.0;
    }
    let bit_len = 256usize - value.leading_zeros();
    if bit_len <= 128 {
        return value.as_u128() as f64;
    }
    let shift = bit_len - 53;
    let significand = (value >> shift).as_u128() as f64;
    significand * 2f64.powi(shift as i32)
}

fn try_mul_div(a: U256, b: U256, denominator: U256) -> Result<U256, MathError> {
    if denominator.is_zero() {
        return Err(MathError::DivisionByZero);
    }
    U256::checked_mul_div(a, b, denominator).ok_or(MathError::MulDivOverflow)
}

fn try_mul_div_ceil(a: U256, b: U256, denominator: U256) -> Result<U256, MathError> {
    if denominator.is_zero() {
        return Err(MathError::DivisionByZero);
    }
    U256::checked_mul_div_ceil(a, b, denominator).ok_or(MathError::MulDivOverflow)
}

fn try_add(a: U256, b: U256) -> Result<U256, MathError> {
    if b > U256::MAX - a {
        return Err(MathError::ArithmeticOverflow);
    }
    Ok(a + b)
}

/// Value token X in token Y at the squared Q64.96 anchor.
///
/// The two nested round-down operations are intentional and exactly match
/// Solidity `_xValueInY`; they must not be collapsed into one division.
pub fn try_x_value_in_y(amount_x: U256, sqrt_price_x96: U256) -> Result<U256, MathError> {
    if !sqrt_price_x96.fits_u160() {
        return Err(MathError::SqrtPriceExceedsUint160);
    }
    let first = try_mul_div(amount_x, sqrt_price_x96, Q96)?;
    try_mul_div(first, sqrt_price_x96, Q96)
}

/// Panicking convenience wrapper around [`try_x_value_in_y`].
#[must_use]
pub fn x_value_in_y(amount_x: U256, sqrt_price_x96: U256) -> U256 {
    try_x_value_in_y(amount_x, sqrt_price_x96).expect("Solidity xValueInY reverted")
}

/// Apply the current Q24 output fee and caller-specific multiplier.
pub fn try_apply_fee(
    gross_output: U256,
    fee_q24: u32,
    fee_multiplier: U256,
) -> Result<(U256, U256), MathError> {
    if fee_q24 > MAX_U24 {
        return Err(MathError::FeeExceedsUint24);
    }
    if fee_q24 == MAX_U24 {
        return Ok((U256::ZERO, gross_output));
    }

    let base_fee = try_mul_div(gross_output, U256::from(fee_q24), U256::from(Q24))?;
    if fee_multiplier <= U256::from(1u64) || base_fee.is_zero() {
        return Ok((gross_output - base_fee, base_fee));
    }
    if fee_multiplier > U256::MAX / base_fee {
        return Ok((U256::ZERO, gross_output));
    }

    let fee = base_fee * fee_multiplier;
    if fee >= gross_output {
        return Ok((U256::ZERO, gross_output));
    }
    Ok((gross_output - fee, fee))
}

/// Panicking convenience wrapper around [`try_apply_fee`].
#[must_use]
pub fn apply_fee(gross_output: U256, fee_q24: u32, fee_multiplier: U256) -> (U256, U256) {
    try_apply_fee(gross_output, fee_q24, fee_multiplier).expect("Solidity applyFee reverted")
}

fn zero_quote(sqrt_price_x96: U256, effective_fee_x24: u32) -> QuoteResult {
    QuoteResult {
        amount_out: U256::ZERO,
        sqrt_price_next: sqrt_price_x96,
        fee: U256::ZERO,
        effective_fee_x24,
    }
}

/// Checked X→Y quote using the effective bid fee (`fee_multiplier == 1`).
pub fn try_quote_x_to_y(params: &PoolParams, dx: U256) -> Result<QuoteResult, MathError> {
    try_quote_x_to_y_with_multiplier(params, dx, U256::from(1u64))
}

/// Checked Y→X quote using the effective ask fee (`fee_multiplier == 1`).
pub fn try_quote_y_to_x(params: &PoolParams, dy: U256) -> Result<QuoteResult, MathError> {
    try_quote_y_to_x_with_multiplier(params, dy, U256::from(1u64))
}

/// Checked X→Y quote with an explicit caller fee multiplier.
pub fn try_quote_x_to_y_with_multiplier(
    params: &PoolParams,
    dx: U256,
    fee_multiplier: U256,
) -> Result<QuoteResult, MathError> {
    params.validate()?;
    let punishment = try_punishment_transition(params, dx, Direction::XToY)?;
    let gross_output = try_x_value_in_y(dx, params.sqrt_price_x96)?;
    if gross_output.is_zero() || gross_output > U256::from(params.reserve_y) {
        return Ok(zero_quote(params.sqrt_price_x96, punishment.fee_bid_x24));
    }
    let (amount_out, fee) = try_apply_fee(gross_output, punishment.fee_bid_x24, fee_multiplier)?;
    Ok(QuoteResult {
        amount_out,
        sqrt_price_next: params.sqrt_price_x96,
        fee,
        effective_fee_x24: punishment.fee_bid_x24,
    })
}

/// Checked Y→X quote with an explicit caller fee multiplier.
pub fn try_quote_y_to_x_with_multiplier(
    params: &PoolParams,
    dy: U256,
    fee_multiplier: U256,
) -> Result<QuoteResult, MathError> {
    params.validate()?;
    let punishment = try_punishment_transition(params, dy, Direction::YToX)?;
    if params.sqrt_price_x96.is_zero() {
        return Ok(zero_quote(params.sqrt_price_x96, punishment.fee_ask_x24));
    }
    let first = try_mul_div(dy, Q96, params.sqrt_price_x96)?;
    let gross_output = try_mul_div(first, Q96, params.sqrt_price_x96)?;
    if gross_output.is_zero() || gross_output > U256::from(params.reserve_x) {
        return Ok(zero_quote(params.sqrt_price_x96, punishment.fee_ask_x24));
    }
    let (amount_out, fee) = try_apply_fee(gross_output, punishment.fee_ask_x24, fee_multiplier)?;
    Ok(QuoteResult {
        amount_out,
        sqrt_price_next: params.sqrt_price_x96,
        fee,
        effective_fee_x24: punishment.fee_ask_x24,
    })
}

/// Panicking convenience wrapper around [`try_quote_x_to_y`].
#[must_use]
pub fn quote_x_to_y(params: &PoolParams, dx: U256) -> QuoteResult {
    try_quote_x_to_y(params, dx).expect("Solidity quoteXToY reverted")
}

/// Panicking convenience wrapper around [`try_quote_y_to_x`].
#[must_use]
pub fn quote_y_to_x(params: &PoolParams, dy: U256) -> QuoteResult {
    try_quote_y_to_x(params, dy).expect("Solidity quoteYToX reverted")
}

/// Panicking convenience wrapper around
/// [`try_quote_x_to_y_with_multiplier`].
#[must_use]
pub fn quote_x_to_y_with_multiplier(
    params: &PoolParams,
    dx: U256,
    fee_multiplier: U256,
) -> QuoteResult {
    try_quote_x_to_y_with_multiplier(params, dx, fee_multiplier)
        .expect("Solidity quoteXToY reverted")
}

/// Panicking convenience wrapper around
/// [`try_quote_y_to_x_with_multiplier`].
#[must_use]
pub fn quote_y_to_x_with_multiplier(
    params: &PoolParams,
    dy: U256,
    fee_multiplier: U256,
) -> QuoteResult {
    try_quote_y_to_x_with_multiplier(params, dy, fee_multiplier)
        .expect("Solidity quoteYToX reverted")
}

/// Compute the desired punishment increment included in the current quote.
///
/// Reserves are the pre-swap active reserves. The `uint24::MAX` maximum is
/// expanded to conceptual Q24 before the single rounded-up ratio operation.
pub fn try_punishment_x24(
    params: &PoolParams,
    amount_in: U256,
    direction: Direction,
) -> Result<u32, MathError> {
    params.validate()?;
    if amount_in.is_zero() || params.max_punishment_x24 == 0 || params.sqrt_price_x96.is_zero() {
        return Ok(0);
    }

    let x_wealth = try_x_value_in_y(U256::from(params.reserve_x), params.sqrt_price_x96)?;
    let inventory_wealth = try_add(x_wealth, U256::from(params.reserve_y))?;
    if inventory_wealth.is_zero() {
        return Ok(0);
    }

    let swap_wealth = match direction {
        Direction::XToY => try_x_value_in_y(amount_in, params.sqrt_price_x96)?,
        Direction::YToX => amount_in,
    };
    if swap_wealth.is_zero() {
        return Ok(0);
    }

    let maximum = if params.max_punishment_x24 == MAX_U24 {
        U256::from(Q24)
    } else {
        U256::from(params.max_punishment_x24)
    };
    let calculated = if swap_wealth >= inventory_wealth {
        maximum
    } else {
        try_mul_div_ceil(maximum, swap_wealth, inventory_wealth)?
    };
    if calculated >= U256::from(Q24) {
        return Ok(MAX_U24);
    }
    Ok(calculated.to::<u32>())
}

/// Panicking convenience wrapper around [`try_punishment_x24`].
#[must_use]
pub fn punishment_x24(params: &PoolParams, amount_in: U256, direction: Direction) -> u32 {
    try_punishment_x24(params, amount_in, direction).expect("Solidity punishmentX24 reverted")
}

/// Saturating-add a desired punishment to the matching directional fee.
///
/// The resulting fee prices the current quote and is persisted only if the
/// surrounding swap transaction succeeds.
pub fn try_apply_punishment(
    params: &PoolParams,
    desired_punishment_x24: u32,
    direction: Direction,
) -> Result<PunishmentTransition, MathError> {
    params.validate()?;
    if desired_punishment_x24 > MAX_U24 {
        return Err(MathError::PunishmentExceedsUint24);
    }

    let current_fee = match direction {
        Direction::XToY => params.fee_bid_x24,
        Direction::YToX => params.fee_ask_x24,
    };
    let available = MAX_U24 - current_fee;
    let applied = desired_punishment_x24.min(available);
    let mut fee_ask_x24 = params.fee_ask_x24;
    let mut fee_bid_x24 = params.fee_bid_x24;
    match direction {
        Direction::XToY => fee_bid_x24 += applied,
        Direction::YToX => fee_ask_x24 += applied,
    }

    Ok(PunishmentTransition {
        desired_punishment_x24,
        applied_punishment_x24: applied,
        fee_ask_x24,
        fee_bid_x24,
    })
}

/// Panicking convenience wrapper around [`try_apply_punishment`].
#[must_use]
pub fn apply_punishment(
    params: &PoolParams,
    desired_punishment_x24: u32,
    direction: Direction,
) -> PunishmentTransition {
    try_apply_punishment(params, desired_punishment_x24, direction)
        .expect("Solidity punishment transition reverted")
}

/// Compute the effective directional fee for one swap amount.
pub fn try_punishment_transition(
    params: &PoolParams,
    amount_in: U256,
    direction: Direction,
) -> Result<PunishmentTransition, MathError> {
    let desired = try_punishment_x24(params, amount_in, direction)?;
    try_apply_punishment(params, desired, direction)
}

/// Mirror an operator `upd`: replace the anchor and both current fees while
/// preserving reserves and the owner-configured punishment maximum.
pub fn try_apply_update(
    params: &PoolParams,
    sqrt_price_x96: U256,
    fee_ask_x24: u32,
    fee_bid_x24: u32,
) -> Result<PoolParams, MathError> {
    let updated = PoolParams {
        sqrt_price_x96,
        fee_ask_x24,
        fee_bid_x24,
        reserve_x: params.reserve_x,
        reserve_y: params.reserve_y,
        max_punishment_x24: params.max_punishment_x24,
    };
    updated.validate()?;
    Ok(updated)
}

/// Panicking convenience wrapper around [`try_apply_update`].
#[must_use]
pub fn apply_update(
    params: &PoolParams,
    sqrt_price_x96: U256,
    fee_ask_x24: u32,
    fee_bid_x24: u32,
) -> PoolParams {
    try_apply_update(params, sqrt_price_x96, fee_ask_x24, fee_bid_x24)
        .expect("Solidity upd validation reverted")
}

fn rolled_back_punishment(
    params: &PoolParams,
    desired_punishment_x24: u32,
) -> PunishmentTransition {
    PunishmentTransition {
        desired_punishment_x24,
        applied_punishment_x24: 0,
        fee_ask_x24: params.fee_ask_x24,
        fee_bid_x24: params.fee_bid_x24,
    }
}

/// Simulate the atomic state effects of one exact-input swap.
///
/// This assumes standard token transfers: the input active reserve increases
/// by `amount_in`, while the output active reserve decreases by
/// `quote.amount_out + quote.fee`. The quote already includes the current
/// punishment; that effective fee is committed only when settlement succeeds.
/// Full-gross reserve subtraction also requires all charged fees to be credited
/// (`partner fee == 0` or a configured partner operator) and adequate uint112
/// treasury/global partner/per-router fee-bucket capacity. These fields are
/// outside [`PoolParams`]; publication can preflight them with
/// [`crate::try_validate_fee_accounting_capacity`].
/// Initial active reserves must also match the token balance partition after
/// pending deposits and global fees; an unsynced donation is not modeled.
/// External transfer failures can be marked afterwards with
/// [`SwapSimulation::mark_rolled_back`].
pub fn try_simulate_successful_swap(
    params: &PoolParams,
    amount_in: U256,
    direction: Direction,
    fee_multiplier: U256,
) -> Result<SwapSimulation, MathError> {
    params.validate()?;
    let punishment = try_punishment_transition(params, amount_in, direction)?;
    let quote = match direction {
        Direction::XToY => try_quote_x_to_y_with_multiplier(params, amount_in, fee_multiplier)?,
        Direction::YToX => try_quote_y_to_x_with_multiplier(params, amount_in, fee_multiplier)?,
    };
    if quote.amount_out.is_zero() {
        return Ok(SwapSimulation {
            quote,
            // The quote includes punishment, but SwapImpossible rolls state back.
            punishment: rolled_back_punishment(params, punishment.desired_punishment_x24),
            pre_swap: *params,
            post_swap: *params,
            status: SimulationStatus::RolledBack(RollbackReason::SwapImpossible),
        });
    }

    let gross_output = try_add(quote.amount_out, quote.fee)?;
    let mut post_swap = *params;
    post_swap.fee_ask_x24 = punishment.fee_ask_x24;
    post_swap.fee_bid_x24 = punishment.fee_bid_x24;

    let reserve_transition_fits = match direction {
        Direction::XToY => {
            if amount_in > U256::from(MAX_U112 - params.reserve_x) {
                false
            } else {
                post_swap.reserve_x = params.reserve_x + amount_in.as_u128();
                post_swap.reserve_y -= gross_output.as_u128();
                true
            }
        }
        Direction::YToX => {
            if amount_in > U256::from(MAX_U112 - params.reserve_y) {
                false
            } else {
                post_swap.reserve_y = params.reserve_y + amount_in.as_u128();
                post_swap.reserve_x -= gross_output.as_u128();
                true
            }
        }
    };

    let (punishment, status) = if reserve_transition_fits {
        (punishment, SimulationStatus::Applied)
    } else {
        post_swap = *params;
        (
            rolled_back_punishment(params, punishment.desired_punishment_x24),
            SimulationStatus::RolledBack(RollbackReason::ReserveTransitionOverflow),
        )
    };
    Ok(SwapSimulation {
        quote,
        punishment,
        pre_swap: *params,
        post_swap,
        status,
    })
}

/// Panicking convenience wrapper around [`try_simulate_successful_swap`].
#[must_use]
pub fn simulate_successful_swap(
    params: &PoolParams,
    amount_in: U256,
    direction: Direction,
    fee_multiplier: U256,
) -> SwapSimulation {
    try_simulate_successful_swap(params, amount_in, direction, fee_multiplier)
        .expect("Solidity swap math reverted")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> PoolParams {
        PoolParams {
            sqrt_price_x96: Q96,
            fee_ask_x24: 0,
            fee_bid_x24: 0,
            reserve_x: 1_000_000,
            reserve_y: 1_000_000,
            max_punishment_x24: 16_778,
        }
    }

    #[test]
    fn validates_full_solidity_widths() {
        let mut p = params();
        p.sqrt_price_x96 = U256::from(1u64) << 159;
        assert_eq!(p.validate(), Ok(()));
        p.sqrt_price_x96 = U256::from(1u64) << 160;
        assert_eq!(p.validate(), Err(MathError::SqrtPriceExceedsUint160));

        p = params();
        p.reserve_x = MAX_U112 + 1;
        assert_eq!(p.validate(), Err(MathError::ReserveXExceedsUint112));
        p = params();
        p.fee_bid_x24 = Q24;
        assert_eq!(p.validate(), Err(MathError::FeeBidExceedsUint24));
        p = params();
        p.max_punishment_x24 = Q24;
        assert_eq!(p.validate(), Err(MathError::MaxPunishmentExceedsUint24));
    }

    #[test]
    fn linear_quotes_use_nested_floor_and_keep_anchor() {
        let mut p = params();
        p.sqrt_price_x96 = Q96 * U256::from(2u64); // raw price = 4
        let x_to_y = try_quote_x_to_y(&p, U256::from(25u64)).unwrap();
        assert_eq!(x_to_y.amount_out, U256::from(100u64));
        assert_eq!(x_to_y.sqrt_price_next, p.sqrt_price_x96);

        let y_to_x = try_quote_y_to_x(&p, U256::from(103u64)).unwrap();
        assert_eq!(y_to_x.amount_out, U256::from(25u64));
        assert_eq!(y_to_x.sqrt_price_next, p.sqrt_price_x96);
    }

    #[test]
    fn directional_fee_and_multiplier_match_solidity() {
        let mut p = params();
        p.max_punishment_x24 = 0;
        p.fee_bid_x24 = Q24 / 100;
        let base = try_quote_x_to_y(&p, U256::from(10_000u64)).unwrap();
        let scaled =
            try_quote_x_to_y_with_multiplier(&p, U256::from(10_000u64), U256::from(3u64)).unwrap();
        assert_eq!(base.fee, U256::from(99u64));
        assert_eq!(scaled.fee, U256::from(297u64));
        assert_eq!(scaled.amount_out, U256::from(9_703u64));
    }

    #[test]
    fn fee_sentinel_and_multiplier_overflow_consume_gross() {
        let (amount_out, fee) =
            try_apply_fee(U256::from(1_000u64), MAX_U24, U256::from(1u64)).unwrap();
        assert_eq!((amount_out, fee), (U256::ZERO, U256::from(1_000u64)));

        let (amount_out, fee) = try_apply_fee(U256::from(1_000u64), Q24 / 100, U256::MAX).unwrap();
        assert_eq!((amount_out, fee), (U256::ZERO, U256::from(1_000u64)));
    }

    #[test]
    fn checked_quote_reports_solidity_mul_div_overflow() {
        let mut p = params();
        p.sqrt_price_x96 = MAX_U160;
        assert_eq!(
            try_quote_x_to_y(&p, U256::MAX),
            Err(MathError::MulDivOverflow)
        );

        p.sqrt_price_x96 = U256::from(1u64);
        assert_eq!(
            try_quote_y_to_x(&p, U256::MAX),
            Err(MathError::MulDivOverflow)
        );
    }

    #[test]
    fn punishment_uses_conceptual_q24_sentinel_and_ceil() {
        let mut p = params();
        p.reserve_x = 450;
        p.reserve_y = 450;
        p.max_punishment_x24 = MAX_U24;
        assert_eq!(
            try_punishment_x24(&p, U256::from(300u64), Direction::XToY).unwrap(),
            5_592_406
        );
    }

    #[test]
    fn punishment_saturates_only_same_direction() {
        let mut p = params();
        p.fee_ask_x24 = 7;
        p.fee_bid_x24 = MAX_U24 - 5;
        let transition = try_apply_punishment(&p, 100, Direction::XToY).unwrap();
        assert_eq!(transition.desired_punishment_x24, 100);
        assert_eq!(transition.applied_punishment_x24, 5);
        assert_eq!(transition.fee_ask_x24, 7);
        assert_eq!(transition.fee_bid_x24, MAX_U24);
    }

    #[test]
    fn operator_update_resets_effective_fees_but_keeps_config_and_reserves() {
        let mut p = params();
        p.fee_ask_x24 = 100;
        p.fee_bid_x24 = 200;
        let updated = try_apply_update(&p, Q96 * U256::from(2u64), 11, 22).unwrap();
        assert_eq!(updated.fee_ask_x24, 11);
        assert_eq!(updated.fee_bid_x24, 22);
        assert_eq!(updated.max_punishment_x24, p.max_punishment_x24);
        assert_eq!(
            (updated.reserve_x, updated.reserve_y),
            (p.reserve_x, p.reserve_y)
        );
    }

    #[test]
    fn simulation_prices_with_immediate_punishment_then_moves_active_reserves() {
        let p = params();
        let simulation = try_simulate_successful_swap(
            &p,
            U256::from(100_000u64),
            Direction::XToY,
            U256::from(1u64),
        )
        .unwrap();
        assert_eq!(simulation.status, SimulationStatus::Applied);
        assert_eq!(simulation.quote.amount_out, U256::from(99_995u64));
        assert_eq!(simulation.quote.fee, U256::from(5u64));
        assert_eq!(simulation.quote.effective_fee_x24, 839);
        assert!(simulation.punishment.applied_punishment_x24 > 0);
        assert_eq!(simulation.post_swap.reserve_x, 1_100_000);
        assert_eq!(simulation.post_swap.reserve_y, 900_000);

        let next_quote =
            try_quote_x_to_y(simulation.effective_params(), U256::from(100_000u64)).unwrap();
        assert!(next_quote.fee > U256::ZERO);
    }

    #[test]
    fn immediate_punishment_that_reaches_sentinel_blocks_without_persisting() {
        let mut p = params();
        p.reserve_x = 0;
        p.reserve_y = 1_000;
        p.fee_bid_x24 = MAX_U24 - 5;
        p.max_punishment_x24 = 5;

        let quote = try_quote_x_to_y(&p, U256::from(1_000u64)).unwrap();
        assert_eq!(quote.amount_out, U256::ZERO);
        assert_eq!(quote.fee, U256::from(1_000u64));
        assert_eq!(quote.effective_fee_x24, MAX_U24);

        let simulation = try_simulate_successful_swap(
            &p,
            U256::from(1_000u64),
            Direction::XToY,
            U256::from(1u64),
        )
        .unwrap();
        assert_eq!(
            simulation.status,
            SimulationStatus::RolledBack(RollbackReason::SwapImpossible)
        );
        assert_eq!(simulation.punishment.desired_punishment_x24, 5);
        assert_eq!(simulation.punishment.applied_punishment_x24, 0);
        assert_eq!(*simulation.effective_params(), p);
    }

    #[test]
    fn full_immediate_punishment_split_regression_matches_solidity() {
        let mut single = params();
        let reserve = 1_000_000_000_000_000_000_000_000u128;
        single.reserve_x = reserve;
        single.reserve_y = reserve;
        single.fee_ask_x24 = 0;
        single.fee_bid_x24 = 0;
        single.max_punishment_x24 = MAX_U24;
        let mut split = single;
        let total = U256::from(reserve);
        let chunk = U256::from(100_000_000_000_000_000_000_000u128);

        let single_execution =
            simulate_successful_swap(&single, total, Direction::XToY, U256::from(1u64));
        let mut split_total_out = U256::ZERO;
        for _ in 0..10 {
            let execution =
                simulate_successful_swap(&split, chunk, Direction::XToY, U256::from(1u64));
            split_total_out += execution.quote.amount_out;
            split = *execution.effective_params();
        }

        assert_eq!(
            single_execution.quote.amount_out,
            U256::from(500_000_000_000_000_000_000_000u128)
        );
        assert_eq!(
            split_total_out,
            U256::from(724_999_934_434_890_747_070_315u128)
        );
        assert_eq!(single_execution.post_swap.fee_bid_x24, 8_388_608);
        assert_eq!(split.fee_bid_x24, 8_388_610);
        assert!(split_total_out > single_execution.quote.amount_out);
    }

    #[test]
    fn simulation_marks_swap_impossible_and_reserve_overflow_rollbacks() {
        let mut p = params();
        p.fee_bid_x24 = MAX_U24;
        let impossible =
            try_simulate_successful_swap(&p, U256::from(1u64), Direction::XToY, U256::from(1u64))
                .unwrap();
        assert_eq!(
            impossible.status,
            SimulationStatus::RolledBack(RollbackReason::SwapImpossible)
        );
        assert_eq!(impossible.punishment.desired_punishment_x24, 1);
        assert_eq!(impossible.punishment.applied_punishment_x24, 0);
        assert_eq!(impossible.punishment.fee_bid_x24, MAX_U24);
        assert_eq!(impossible.post_swap, p);

        p.fee_bid_x24 = 0;
        p.reserve_x = MAX_U112;
        let overflow =
            try_simulate_successful_swap(&p, U256::from(1u64), Direction::XToY, U256::from(1u64))
                .unwrap();
        assert_eq!(
            overflow.status,
            SimulationStatus::RolledBack(RollbackReason::ReserveTransitionOverflow)
        );
        assert!(overflow.punishment.desired_punishment_x24 > 0);
        assert_eq!(overflow.punishment.applied_punishment_x24, 0);
        assert_eq!(*overflow.effective_params(), p);
    }

    #[test]
    fn later_failure_can_restore_atomic_snapshot() {
        let p = params();
        let mut simulation =
            simulate_successful_swap(&p, U256::from(10u64), Direction::YToX, U256::from(1u64));
        assert_eq!(simulation.status, SimulationStatus::Applied);
        assert!(simulation.punishment.applied_punishment_x24 > 0);
        simulation.mark_rolled_back(RollbackReason::LaterRevert);
        assert_eq!(simulation.punishment.applied_punishment_x24, 0);
        assert_eq!(*simulation.effective_params(), p);
        assert_eq!(simulation.post_swap, p);
    }

    #[test]
    fn price_converter_supports_full_uint160_container() {
        assert_eq!(price_to_sqrt_price_x96(1.0), Q96);
        assert_eq!(sqrt_price_x96_to_price(Q96), 1.0);
        assert!(MAX_U160.fits_u160());
    }
}
