//! Strict N-API boundary for the bit-exact LunarBase punishment math.
#![allow(
    missing_docs,
    clippy::needless_pass_by_value,
    clippy::missing_safety_doc
)]

use lunarbase_pmm_math::{
    geometric_sizes as internal_geometric_sizes, price_to_sqrt_price_x96, sqrt_price_x96_to_price,
    try_build_order_book, try_build_precise_order_book, try_build_validated_order_book,
    try_ladder_amount_out_at_cursor, try_quote_x_to_y_with_multiplier,
    try_quote_y_to_x_with_multiplier, try_simulate_successful_swap,
    try_validate_fee_accounting_capacity, Direction,
    DirectionalLadder as InternalDirectionalLadder,
    FeeAccountingState as InternalFeeAccountingState, FillPolicy as InternalFillPolicy, MathError,
    OrderBook as InternalOrderBook, OrderBookConfig, OrderBookError,
    OrderBookLevel as InternalOrderBookLevel, OrderBookPrecision as InternalOrderBookPrecision,
    OrderBookSafety as InternalOrderBookSafety, OrderBookState,
    OrderBookStatus as InternalOrderBookStatus, PoolParams, QuoteResult as InternalQuoteResult,
    RollbackReason, SimulationStatus, U256Ext,
    ValidatedOrderBookConfig as InternalValidatedOrderBookConfig, MAX_ORDER_BOOK_LEVELS,
    MAX_PRECISION_WORK, MAX_U24, MAX_VALIDATION_TRANSITIONS, PARTNER_FEE_SCALE, U256,
};
use napi::bindgen_prelude::*;
use napi_derive::napi;

const MAX_U256_DECIMAL_DIGITS: usize = 78;

fn math_error(error: MathError) -> Error {
    Error::from_reason(error.to_string())
}

fn parse_u256(value: &str) -> Result<U256> {
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        return parse_hex_u256(hex);
    }
    parse_decimal_u256(value)
}

fn parse_hex_u256(hex: &str) -> Result<U256> {
    if hex.is_empty() {
        return Err(Error::from_reason("empty hexadecimal integer"));
    }
    if hex.len() > 64 {
        return Err(Error::from_reason(
            "hexadecimal integer exceeds the 64-digit uint256 limit",
        ));
    }

    let mut result = U256::ZERO;
    for byte in hex.bytes() {
        let digit = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => return Err(Error::from_reason("invalid hexadecimal integer")),
        };
        result = result
            .checked_mul(U256::from(16u64))
            .and_then(|current| current.checked_add(U256::from(digit)))
            .ok_or_else(|| Error::from_reason("hexadecimal integer exceeds uint256 range"))?;
    }
    Ok(result)
}

fn parse_decimal_u256(value: &str) -> Result<U256> {
    if value.is_empty() {
        return Err(Error::from_reason("empty decimal integer"));
    }
    if value.len() > MAX_U256_DECIMAL_DIGITS {
        return Err(Error::from_reason(
            "decimal integer exceeds the 78-digit uint256 limit",
        ));
    }
    if value.len() > 1 && value.starts_with('0') {
        return Err(Error::from_reason(
            "decimal integer must be canonical and contain no leading zeros",
        ));
    }

    let mut result = U256::ZERO;
    for byte in value.bytes() {
        if !byte.is_ascii_digit() {
            return Err(Error::from_reason("invalid decimal integer"));
        }
        result = result
            .checked_mul(U256::from(10u64))
            .and_then(|current| current.checked_add(U256::from(byte - b'0')))
            .ok_or_else(|| Error::from_reason("decimal integer exceeds uint256 range"))?;
    }
    Ok(result)
}

fn parse_u112_field(name: &str, value: &str) -> Result<u128> {
    let parsed = parse_u256(value)?;
    if !parsed.fits_u112() {
        return Err(Error::from_reason(format!(
            "{name} exceeds uint112 range: {value}"
        )));
    }
    Ok(parsed.as_u128())
}

fn parse_u64_field(name: &str, value: &str) -> Result<u64> {
    let parsed = parse_u256(value)?;
    if parsed > U256::from(u64::MAX) {
        return Err(Error::from_reason(format!(
            "{name} exceeds uint64 range: {value}"
        )));
    }
    Ok(parsed.to::<u64>())
}

/// N-API must receive Q24 values as an uncoerced JS number. Using `f64`
/// deliberately avoids modulo/truncation coercion before Rust validation.
fn parse_u24_field(name: &str, value: f64) -> Result<u32> {
    if !value.is_finite() || value.fract() != 0.0 || value < 0.0 || value > f64::from(MAX_U24) {
        return Err(Error::from_reason(format!(
            "{name} must be an integer in [0, {MAX_U24}]"
        )));
    }
    Ok(value as u32)
}

#[napi(object)]
pub struct QuoteParams {
    /// Operator anchor sqrt-price (Q64.96 uint160), decimal or 0x-prefixed hex.
    pub sqrt_price_x96: String,
    /// Current Y -> X fee in Q24.
    pub fee_ask_x24: f64,
    /// Current X -> Y fee in Q24.
    pub fee_bid_x24: f64,
    /// Active token-X reserve (uint112), decimal or hex.
    pub reserve_x: String,
    /// Active token-Y reserve (uint112), decimal or hex.
    pub reserve_y: String,
    /// Maximum immediate directional punishment in Q24. Defaults to disabled.
    pub max_punishment_x24: Option<f64>,
    /// Exact input amount (uint256), decimal or hex.
    pub amount_in: String,
    /// Caller-specific fee multiplier (uint256). Defaults to one.
    pub fee_multiplier: Option<String>,
}

#[napi(object)]
pub struct QuoteResult {
    pub amount_out: String,
    /// Always equals the operator anchor in the linear implementation.
    pub sqrt_price_next: String,
    pub fee: String,
    /// Saturating directional fee used by this quote before feeMultiplier.
    pub effective_fee_x24: u32,
}

#[napi(object)]
pub struct BuildOrderBookParams {
    /// Operator anchor sqrt-price (Q64.96 uint160), decimal or 0x-prefixed hex.
    pub sqrt_price_x96: String,
    /// Current Y -> X fee in Q24.
    pub fee_ask_x24: f64,
    /// Current X -> Y fee in Q24.
    pub fee_bid_x24: f64,
    /// Active token-X reserve (uint112), decimal or hex.
    pub reserve_x: String,
    /// Active token-Y reserve (uint112), decimal or hex.
    pub reserve_y: String,
    /// Maximum immediate directional punishment in Q24.
    pub max_punishment_x24: f64,
    /// Resolved fee multiplier of the address that will call the Pool.
    pub fee_multiplier: String,
    /// Block number of the complete cached snapshot.
    pub snapshot_block: String,
    /// Latest block in which the publisher will allow this ladder to execute.
    pub max_execution_block: String,
    /// Block of the most recent operator update (uint48 on-chain).
    pub latest_update_block: String,
    /// Operator-state freshness window (uint48 on-chain).
    pub block_delay: String,
    /// Pause state from the same cached snapshot.
    pub paused: bool,
    /// Cumulative raw token-X input sizes for X -> Y; empty disables the side.
    pub x_to_y_sizes: Vec<String>,
    /// Cumulative raw token-Y input sizes for Y -> X; empty disables the side.
    pub y_to_x_sizes: Vec<String>,
}

/// Complete coherent snapshot for a policy-validated book.
#[napi(object)]
pub struct OrderBookSnapshot {
    pub sqrt_price_x96: String,
    pub fee_ask_x24: f64,
    pub fee_bid_x24: f64,
    pub reserve_x: String,
    pub reserve_y: String,
    pub max_punishment_x24: f64,
    /// Fee multiplier of the exact adapter calling the Pool.
    pub fee_multiplier: String,
    pub snapshot_block: String,
    /// Latest block covered by the signed lifetime.
    pub max_execution_block: String,
    pub latest_update_block: String,
    pub block_delay: String,
    pub paused: bool,
}

/// Must match the on-chain adapter and signed directional cap exactly.
#[napi(object)]
pub struct FillPolicy {
    pub min_input: String,
    pub lot_input: String,
    pub max_input: String,
    pub total_input: String,
}

#[napi(object)]
pub struct ValidatedOrderBookConfig {
    pub x_to_y: Option<FillPolicy>,
    pub y_to_x: Option<FillPolicy>,
    /// Integer in 1..=100000. Exhaustion throws; no partial certificate.
    pub max_transitions: f64,
}

/// Coherent fee state for the exact adapter/router that calls the Pool.
#[napi(object)]
pub struct FeeAccountingState {
    /// Partner share scaled by 1e6 (Pool FeeManager.BPS), not Q24.
    pub partner_fee: f64,
    pub partner_operator_present: bool,
    pub treasury_x: String,
    pub treasury_y: String,
    pub partner_x: String,
    pub partner_y: String,
    pub router_partner_x: String,
    pub router_partner_y: String,
}

#[napi(object)]
pub struct ValidatedOrderBookResult {
    pub book: OrderBookResult,
    pub snapshot: OrderBookSnapshot,
    pub config: ValidatedOrderBookConfig,
    pub checked_states: u32,
    pub checked_transitions: u32,
}

/// Controls conservative multilevel fitting after exhaustive lot-policy validation.
#[napi(object)]
pub struct OrderBookPrecision {
    /// Maximum directional level count, an integer in 1..=20.
    pub max_levels: f64,
    /// Target maximum underquote against the WORST reachable state for the
    /// same cursor and amount. Integer basis points in 0..=10000.
    pub target_underquote_bps: f64,
    /// Separate bounded fitting-work budget, an integer in 1..=5000000.
    /// Exhaustion throws instead of returning an unchecked approximation.
    pub max_work: f64,
}

#[napi(object)]
pub struct PreciseOrderBookResult {
    /// The same exhaustive policy certificate, now with a fitted multilevel book.
    pub validated: ValidatedOrderBookResult,
    pub precision: OrderBookPrecision,
    /// Measured result, not a promise that the requested tolerance is attainable.
    /// False for inactive snapshots or when the fitted book misses the target.
    pub target_met: bool,
    /// Worst ceiling-rounded underquote in bps against the minimum Pool output
    /// over all reachable states for the same direction, cursor and fill size.
    pub worst_underquote_bps: u32,
    /// Separate discount against the initial snapshot's same-size quote.
    /// This can exceed targetUnderquoteBps even when targetMet is true.
    pub worst_fresh_snapshot_discount_bps: u32,
    pub work_used: u32,
    pub constraint_count: u32,
}

#[napi(string_enum = "camelCase")]
pub enum OrderBookStatus {
    Active,
    Paused,
    Stale,
}

#[napi(string_enum = "camelCase")]
pub enum OrderBookSafety {
    Indicative,
    ExhaustiveLotPolicy,
}

#[napi(object)]
pub struct OrderBookLevel {
    /// Cumulative raw token-in size.
    pub size: String,
    /// Marginal raw token-out/token-in price scaled by 1e18.
    pub price: String,
}

#[napi(object)]
pub struct DirectionalLadderResult {
    pub levels: Vec<OrderBookLevel>,
    pub truncated: bool,
}

#[napi(object)]
pub struct OrderBookResult {
    /// Only active results may contain executable liquidity; also require a
    /// non-empty directional `levels` array before publishing that side.
    pub status: OrderBookStatus,
    /// Mathematical coverage, independent of freshness. An exhaustive lot
    /// policy covers only its snapshot and modeled mixed-direction sequences.
    pub safety: OrderBookSafety,
    /// Echo of the input snapshot block for version/coherence checks.
    pub snapshot_block: String,
    /// Latest execution block covered by the freshness gate.
    pub max_execution_block: String,
    /// True because arbitrary partial/cursor fills require an adapter-side
    /// Pool `amountOutMinimum` guard.
    pub requires_amount_out_minimum: bool,
    pub x_to_y: DirectionalLadderResult,
    pub y_to_x: DirectionalLadderResult,
}

#[napi(string_enum = "camelCase")]
pub enum SwapSimulationStatus {
    Applied,
    SwapImpossible,
    ReserveTransitionOverflow,
    LaterRevert,
}

#[napi(object)]
pub struct SwapSimulationResult {
    pub amount_out: String,
    pub sqrt_price_next: String,
    pub fee: String,
    /// Saturating directional fee used by the current quote before multiplier.
    pub effective_fee_x24: u32,
    /// True when the pure-math quote is nonzero and the standard-token uint112
    /// reserve transition fits. It does not predict external call failures.
    pub executable: bool,
    /// Pure-model outcome. Applied does not guarantee an on-chain transaction
    /// cannot later revert during token transfer or accounting.
    pub status: SwapSimulationStatus,
    pub desired_punishment_x24: u32,
    pub applied_punishment_x24: u32,
    pub fee_ask_x24_after: u32,
    pub fee_bid_x24_after: u32,
    pub reserve_x_after: String,
    pub reserve_y_after: String,
}

fn to_pool_params(params: &QuoteParams) -> Result<(PoolParams, U256, U256)> {
    let sqrt_price_x96 = parse_u256(&params.sqrt_price_x96)?;
    let fee_ask_x24 = parse_u24_field("feeAskX24", params.fee_ask_x24)?;
    let fee_bid_x24 = parse_u24_field("feeBidX24", params.fee_bid_x24)?;
    let reserve_x = parse_u112_field("reserveX", &params.reserve_x)?;
    let reserve_y = parse_u112_field("reserveY", &params.reserve_y)?;
    let max_punishment_x24 =
        parse_u24_field("maxPunishmentX24", params.max_punishment_x24.unwrap_or(0.0))?;
    let amount_in = parse_u256(&params.amount_in)?;
    let fee_multiplier = params
        .fee_multiplier
        .as_deref()
        .map(parse_u256)
        .transpose()?
        .unwrap_or(U256::from(1u64));

    let pool = PoolParams {
        sqrt_price_x96,
        fee_ask_x24,
        fee_bid_x24,
        reserve_x,
        reserve_y,
        max_punishment_x24,
    };
    pool.validate().map_err(math_error)?;
    Ok((pool, amount_in, fee_multiplier))
}

fn to_order_book_state(params: &BuildOrderBookParams) -> Result<OrderBookState> {
    to_order_book_snapshot(&OrderBookSnapshot {
        sqrt_price_x96: params.sqrt_price_x96.clone(),
        fee_ask_x24: params.fee_ask_x24,
        fee_bid_x24: params.fee_bid_x24,
        reserve_x: params.reserve_x.clone(),
        reserve_y: params.reserve_y.clone(),
        max_punishment_x24: params.max_punishment_x24,
        fee_multiplier: params.fee_multiplier.clone(),
        snapshot_block: params.snapshot_block.clone(),
        max_execution_block: params.max_execution_block.clone(),
        latest_update_block: params.latest_update_block.clone(),
        block_delay: params.block_delay.clone(),
        paused: params.paused,
    })
}

fn to_order_book_snapshot(params: &OrderBookSnapshot) -> Result<OrderBookState> {
    let pool = PoolParams {
        sqrt_price_x96: parse_u256(&params.sqrt_price_x96)?,
        fee_ask_x24: parse_u24_field("feeAskX24", params.fee_ask_x24)?,
        fee_bid_x24: parse_u24_field("feeBidX24", params.fee_bid_x24)?,
        reserve_x: parse_u112_field("reserveX", &params.reserve_x)?,
        reserve_y: parse_u112_field("reserveY", &params.reserve_y)?,
        max_punishment_x24: parse_u24_field("maxPunishmentX24", params.max_punishment_x24)?,
    };
    pool.validate().map_err(math_error)?;

    Ok(OrderBookState {
        pool,
        fee_multiplier: parse_u256(&params.fee_multiplier)?,
        snapshot_block: parse_u64_field("snapshotBlock", &params.snapshot_block)?,
        max_execution_block: parse_u64_field("maxExecutionBlock", &params.max_execution_block)?,
        latest_update_block: parse_u64_field("latestUpdateBlock", &params.latest_update_block)?,
        block_delay: parse_u64_field("blockDelay", &params.block_delay)?,
        paused: params.paused,
    })
}

fn parse_sizes(name: &str, values: &[String]) -> Result<Vec<U256>> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            parse_u256(value)
                .map_err(|error| Error::from_reason(format!("{name}[{index}] is invalid: {error}")))
        })
        .collect()
}

fn order_book_error(error: OrderBookError) -> Error {
    Error::from_reason(error.to_string())
}

fn from_order_book_status(status: InternalOrderBookStatus) -> OrderBookStatus {
    match status {
        InternalOrderBookStatus::Active => OrderBookStatus::Active,
        InternalOrderBookStatus::Paused => OrderBookStatus::Paused,
        InternalOrderBookStatus::Stale => OrderBookStatus::Stale,
    }
}

fn from_directional_ladder(ladder: InternalDirectionalLadder) -> DirectionalLadderResult {
    DirectionalLadderResult {
        levels: ladder
            .levels
            .into_iter()
            .map(|level| OrderBookLevel {
                size: level.size.to_string(),
                price: level.price.to_string(),
            })
            .collect(),
        truncated: ladder.truncated,
    }
}

fn from_order_book(book: InternalOrderBook) -> OrderBookResult {
    OrderBookResult {
        status: from_order_book_status(book.status),
        safety: match book.safety {
            InternalOrderBookSafety::Indicative => OrderBookSafety::Indicative,
            InternalOrderBookSafety::ExhaustiveLotPolicy => OrderBookSafety::ExhaustiveLotPolicy,
        },
        snapshot_block: book.snapshot_block.to_string(),
        max_execution_block: book.max_execution_block.to_string(),
        requires_amount_out_minimum: book.requires_amount_out_minimum,
        x_to_y: from_directional_ladder(book.x_to_y),
        y_to_x: from_directional_ladder(book.y_to_x),
    }
}

fn to_fill_policy(policy: &FillPolicy) -> Result<InternalFillPolicy> {
    Ok(InternalFillPolicy {
        min_input: parse_u256(&policy.min_input)?,
        lot_input: parse_u256(&policy.lot_input)?,
        max_input: parse_u256(&policy.max_input)?,
        total_input: parse_u256(&policy.total_input)?,
    })
}

fn from_quote(result: InternalQuoteResult) -> QuoteResult {
    QuoteResult {
        amount_out: result.amount_out.to_string(),
        sqrt_price_next: result.sqrt_price_next.to_string(),
        fee: result.fee.to_string(),
        effective_fee_x24: result.effective_fee_x24,
    }
}

fn quote(params: &QuoteParams, direction: Direction) -> Result<QuoteResult> {
    let (pool, amount_in, fee_multiplier) = to_pool_params(params)?;
    let result = match direction {
        Direction::XToY => try_quote_x_to_y_with_multiplier(&pool, amount_in, fee_multiplier),
        Direction::YToX => try_quote_y_to_x_with_multiplier(&pool, amount_in, fee_multiplier),
    }
    .map_err(math_error)?;
    Ok(from_quote(result))
}

fn rollback_status(reason: &RollbackReason) -> SwapSimulationStatus {
    match reason {
        RollbackReason::SwapImpossible => SwapSimulationStatus::SwapImpossible,
        RollbackReason::ReserveTransitionOverflow => {
            SwapSimulationStatus::ReserveTransitionOverflow
        }
        RollbackReason::LaterRevert => SwapSimulationStatus::LaterRevert,
    }
}

fn simulate(params: &QuoteParams, direction: Direction) -> Result<SwapSimulationResult> {
    let (pool, amount_in, fee_multiplier) = to_pool_params(params)?;
    let simulation = try_simulate_successful_swap(&pool, amount_in, direction, fee_multiplier)
        .map_err(math_error)?;
    let (executable, status) = match &simulation.status {
        SimulationStatus::Applied => (true, SwapSimulationStatus::Applied),
        SimulationStatus::RolledBack(reason) => (false, rollback_status(reason)),
    };
    let effective = simulation.effective_params();

    Ok(SwapSimulationResult {
        amount_out: simulation.quote.amount_out.to_string(),
        sqrt_price_next: simulation.quote.sqrt_price_next.to_string(),
        fee: simulation.quote.fee.to_string(),
        effective_fee_x24: simulation.quote.effective_fee_x24,
        executable,
        status,
        desired_punishment_x24: simulation.punishment.desired_punishment_x24,
        applied_punishment_x24: simulation.punishment.applied_punishment_x24,
        fee_ask_x24_after: effective.fee_ask_x24,
        fee_bid_x24_after: effective.fee_bid_x24,
        reserve_x_after: effective.reserve_x.to_string(),
        reserve_y_after: effective.reserve_y.to_string(),
    })
}

#[napi(js_name = "quoteXToY")]
pub fn quote_x_to_y_napi(params: QuoteParams) -> Result<QuoteResult> {
    quote(&params, Direction::XToY)
}

#[napi(js_name = "quoteYToX")]
pub fn quote_y_to_x_napi(params: QuoteParams) -> Result<QuoteResult> {
    quote(&params, Direction::YToX)
}

#[napi(js_name = "simulateXToY")]
pub fn simulate_x_to_y_napi(params: QuoteParams) -> Result<SwapSimulationResult> {
    simulate(&params, Direction::XToY)
}

#[napi(js_name = "simulateYToX")]
pub fn simulate_y_to_x_napi(params: QuoteParams) -> Result<SwapSimulationResult> {
    simulate(&params, Direction::YToX)
}

#[napi(js_name = "buildOrderBook")]
pub fn build_order_book_napi(params: BuildOrderBookParams) -> Result<OrderBookResult> {
    let state = to_order_book_state(&params)?;
    let x_to_y_sizes = parse_sizes("xToYSizes", &params.x_to_y_sizes)?;
    let y_to_x_sizes = parse_sizes("yToXSizes", &params.y_to_x_sizes)?;
    let book = try_build_order_book(
        &state,
        &OrderBookConfig {
            x_to_y_sizes: &x_to_y_sizes,
            y_to_x_sizes: &y_to_x_sizes,
        },
    )
    .map_err(order_book_error)?;

    Ok(from_order_book(book))
}

/// Certify the finite quote/reserve/punishment model, including mixed-direction
/// fills. First call validateFeeAccountingCapacity on the same snapshot; this
/// does not guarantee arbitrary ERC20 or EVM execution.
#[napi(js_name = "buildValidatedOrderBook")]
pub fn build_validated_order_book_napi(
    snapshot: OrderBookSnapshot,
    config: ValidatedOrderBookConfig,
) -> Result<ValidatedOrderBookResult> {
    let state = to_order_book_snapshot(&snapshot)?;
    let internal_config = to_validated_config(&config)?;
    let validated = try_build_validated_order_book(&state, &internal_config)
        .map_err(|error| Error::from_reason(error.to_string()))?;
    Ok(ValidatedOrderBookResult {
        book: from_order_book(validated.book),
        snapshot,
        config,
        checked_states: validated.checked_states as u32,
        checked_transitions: validated.checked_transitions as u32,
    })
}

/// Fit up to 20 conservative levels against every allowed cursor/interleaving
/// constraint. targetUnderquoteBps references the worst admissible state, not
/// an unconditional fresh-snapshot quote. Validate fee accounting separately.
#[napi(js_name = "buildPreciseOrderBook")]
pub fn build_precise_order_book_napi(
    snapshot: OrderBookSnapshot,
    config: ValidatedOrderBookConfig,
    precision: OrderBookPrecision,
) -> Result<PreciseOrderBookResult> {
    let state = to_order_book_snapshot(&snapshot)?;
    let internal_config = to_validated_config(&config)?;
    let internal_precision = to_order_book_precision(&precision)?;
    let result = try_build_precise_order_book(&state, &internal_config, &internal_precision)
        .map_err(|error| Error::from_reason(error.to_string()))?;
    Ok(PreciseOrderBookResult {
        validated: ValidatedOrderBookResult {
            book: from_order_book(result.validated.book),
            snapshot,
            config,
            checked_states: result.validated.checked_states as u32,
            checked_transitions: result.validated.checked_transitions as u32,
        },
        precision,
        target_met: result.target_met,
        worst_underquote_bps: result.worst_underquote_bps,
        worst_fresh_snapshot_discount_bps: result.worst_fresh_snapshot_discount_bps,
        work_used: result.work_used as u32,
        constraint_count: result.constraint_count as u32,
    })
}

fn to_order_book_precision(precision: &OrderBookPrecision) -> Result<InternalOrderBookPrecision> {
    fn bounded_integer(name: &str, value: f64, minimum: usize, maximum: usize) -> Result<usize> {
        if !value.is_finite()
            || value.fract() != 0.0
            || value < minimum as f64
            || value > maximum as f64
        {
            return Err(Error::from_reason(format!(
                "{name} must be an integer in [{minimum}, {maximum}]"
            )));
        }
        Ok(value as usize)
    }
    Ok(InternalOrderBookPrecision {
        max_levels: bounded_integer("maxLevels", precision.max_levels, 1, MAX_ORDER_BOOK_LEVELS)?,
        target_underquote_bps: bounded_integer(
            "targetUnderquoteBps",
            precision.target_underquote_bps,
            0,
            10_000,
        )? as u32,
        max_work: bounded_integer("maxWork", precision.max_work, 1, MAX_PRECISION_WORK)?,
    })
}

fn to_validated_config(
    config: &ValidatedOrderBookConfig,
) -> Result<InternalValidatedOrderBookConfig> {
    if !config.max_transitions.is_finite()
        || config.max_transitions.fract() != 0.0
        || config.max_transitions < 1.0
        || config.max_transitions > MAX_VALIDATION_TRANSITIONS as f64
    {
        return Err(Error::from_reason(format!(
            "maxTransitions must be an integer in [1, {MAX_VALIDATION_TRANSITIONS}]"
        )));
    }
    Ok(InternalValidatedOrderBookConfig {
        x_to_y: config.x_to_y.as_ref().map(to_fill_policy).transpose()?,
        y_to_x: config.y_to_x.as_ref().map(to_fill_policy).transpose()?,
        max_transitions: config.max_transitions as usize,
    })
}

/// Throws if fees would be left uncredited or conservative uint112 bucket
/// headroom cannot be established for the signed input caps.
#[napi(js_name = "validateFeeAccountingCapacity")]
pub fn validate_fee_accounting_capacity_napi(
    snapshot: OrderBookSnapshot,
    config: ValidatedOrderBookConfig,
    accounting: FeeAccountingState,
) -> Result<()> {
    if !accounting.partner_fee.is_finite()
        || accounting.partner_fee.fract() != 0.0
        || accounting.partner_fee < 0.0
        || accounting.partner_fee > f64::from(PARTNER_FEE_SCALE)
    {
        return Err(Error::from_reason(
            "partnerFee must be an integer in [0, 1000000]",
        ));
    }
    let state = to_order_book_snapshot(&snapshot)?;
    let config = to_validated_config(&config)?;
    let accounting = InternalFeeAccountingState {
        partner_fee: accounting.partner_fee as u32,
        partner_operator_present: accounting.partner_operator_present,
        treasury_x: parse_u112_field("treasuryX", &accounting.treasury_x)?,
        treasury_y: parse_u112_field("treasuryY", &accounting.treasury_y)?,
        partner_x: parse_u112_field("partnerX", &accounting.partner_x)?,
        partner_y: parse_u112_field("partnerY", &accounting.partner_y)?,
        router_partner_x: parse_u112_field("routerPartnerX", &accounting.router_partner_x)?,
        router_partner_y: parse_u112_field("routerPartnerY", &accounting.router_partner_y)?,
    };
    try_validate_fee_accounting_capacity(&state, &config, &accounting)
        .map_err(|error| Error::from_reason(error.to_string()))
}

/// Exact per-tranche floor sum; null when the requested cursor/fill exceeds depth.
#[napi(js_name = "ladderAmountOut")]
pub fn ladder_amount_out_napi(
    levels: Vec<OrderBookLevel>,
    amount_in: String,
    cursor: Option<String>,
) -> Result<Option<String>> {
    if levels.len() > MAX_ORDER_BOOK_LEVELS {
        return Err(order_book_error(OrderBookError::TooManyLadderLevels));
    }
    let ladder = InternalDirectionalLadder {
        levels: levels
            .iter()
            .map(|level| {
                Ok(InternalOrderBookLevel {
                    size: parse_u256(&level.size)?,
                    price: parse_u256(&level.price)?,
                })
            })
            .collect::<Result<Vec<_>>>()?,
        truncated: false,
    };
    try_ladder_amount_out_at_cursor(
        &ladder,
        cursor
            .as_deref()
            .map(parse_u256)
            .transpose()?
            .unwrap_or(U256::ZERO),
        parse_u256(&amount_in)?,
    )
    .map_err(order_book_error)
    .map(|output| output.map(|value| value.to_string()))
}

#[napi(js_name = "geometricSizes")]
pub fn geometric_sizes_napi(cap: String, levels: f64) -> Result<Vec<String>> {
    if !levels.is_finite()
        || levels.fract() != 0.0
        || levels < 1.0
        || levels > MAX_ORDER_BOOK_LEVELS as f64
    {
        return Err(Error::from_reason(format!(
            "levels must be an integer in [1, {MAX_ORDER_BOOK_LEVELS}]"
        )));
    }
    internal_geometric_sizes(parse_u256(&cap)?, levels as usize)
        .map_err(order_book_error)
        .map(|sizes| sizes.into_iter().map(|size| size.to_string()).collect())
}

#[napi(js_name = "priceToSqrtPriceX96")]
pub fn price_to_sqrt_price_x96_napi(price: f64) -> Result<String> {
    if !price.is_finite() || price < 0.0 {
        return Err(Error::from_reason(
            "price must be finite and non-negative".to_owned(),
        ));
    }
    Ok(price_to_sqrt_price_x96(price).to_string())
}

#[napi(js_name = "price_to_sqrt_price_x96")]
pub fn price_to_sqrt_price_x96_compat_napi(price: f64) -> Result<String> {
    price_to_sqrt_price_x96_napi(price)
}

#[napi(js_name = "sqrtPriceX96ToPrice")]
pub fn sqrt_price_x96_to_price_napi(value: String) -> Result<f64> {
    let parsed = parse_u256(&value)?;
    if !parsed.fits_u160() {
        return Err(Error::from_reason(format!(
            "sqrtPriceX96 exceeds uint160 range: {value}"
        )));
    }
    Ok(sqrt_price_x96_to_price(parsed))
}

#[napi(js_name = "sqrt_price_x96_to_price")]
pub fn sqrt_price_x96_to_price_compat_napi(value: String) -> Result<f64> {
    sqrt_price_x96_to_price_napi(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_u256_parser_accepts_only_bounded_canonical_values() {
        assert_eq!(parse_decimal_u256("0").unwrap(), U256::ZERO);
        assert_eq!(
            parse_decimal_u256(&U256::MAX.to_string()).unwrap(),
            U256::MAX
        );

        assert!(parse_decimal_u256("00").is_err());
        assert!(parse_decimal_u256("01").is_err());
        assert!(parse_decimal_u256(&"1".repeat(79)).is_err());
        assert!(parse_decimal_u256(
            "115792089237316195423570985008687907853269984665640564039457584007913129639936"
        )
        .is_err());
    }
}
