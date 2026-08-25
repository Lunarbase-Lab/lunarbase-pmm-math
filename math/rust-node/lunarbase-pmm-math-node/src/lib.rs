//! Strict N-API boundary for the bit-exact LunarBase punishment math.
#![allow(
    missing_docs,
    clippy::needless_pass_by_value,
    clippy::missing_safety_doc
)]

use lunarbase_pmm_math::{
    price_to_sqrt_price_x96, sqrt_price_x96_to_price, try_quote_x_to_y_with_multiplier,
    try_quote_y_to_x_with_multiplier, try_simulate_successful_swap, Direction, MathError,
    PoolParams, QuoteResult as InternalQuoteResult, RollbackReason, SimulationStatus, U256Ext,
    MAX_U24, U256,
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
