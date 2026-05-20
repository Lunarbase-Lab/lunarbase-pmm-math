//! Direct port of the Solidity `SwapLib` quoting library on `dev`
//! (post-`update/asymetric-fees` merge).
//!
//! Differences from the previous (single-fee Q48) implementation:
//!
//! * Single Q48 `fee` is replaced by directional Q24 fees: `fee_bid_x24`
//!   (charged on X→Y) and `fee_ask_x24` (charged on Y→X).
//! * `concentration_k` is stored in Q20.12 fixed-point: effective
//!   `K = concentration_k / 2^12`. The numeric value of the old plain-int
//!   `concentrationK = X` is preserved by storing `concentration_k =
//!   X << 12` (i.e. `X * 4096`). Field name matches the on-chain
//!   `concentrationK` after the `concentrationK revert` commit.
//! * Concentration is no longer mixed with the base fee. Pure form:
//!   `cQ48 = mulDiv(concentrationK, r², Q12)` with wealth normalised by
//!   `anchorPrice`.
//! * When `cQ48 == 0` the swap takes a linear-fallback path using the
//!   stored price as a flat constant; `pNext` is left at `pX96`.
//! * `fee_multiplier` (≥ 1) scales the base fee per-call. The dev contract
//!   uses this for the whitelist gate (whitelisted = 1, others = configured
//!   multiplier). When multiplier ≤ 1 or the scaled fee >= grossOutput, the
//!   swap is rejected (amount_out = 0, fee = grossOutput) — mirrors
//!   `applyFee` exactly.

use crate::sqrt_price_math::{
    get_amount_x_delta, get_amount_y_delta, get_next_sqrt_price_from_amount_x_rounding_up,
    get_next_sqrt_price_from_amount_y_rounding_down,
};
use crate::uint256::{U256Ext, U256};

/// Q48 fixed-point unit (`2^48`).
pub const Q48: u128 = 1u128 << 48;
/// Q24 fixed-point unit (`2^24`).
pub const Q24: u128 = 1u128 << 24;
/// Q12 fixed-point unit (`2^12`).
pub const Q12: u128 = 1u128 << 12;

const Q48_U256: U256 = U256::Q48;
const Q24_U256: U256 = {
    let mut limbs = [0u64; 4];
    limbs[0] = 1u64 << 24;
    U256::from_limbs(limbs)
};
const Q12_U256: U256 = {
    let mut limbs = [0u64; 4];
    limbs[0] = 1u64 << 12;
    U256::from_limbs(limbs)
};
const Q96_U256: U256 = {
    let mut limbs = [0u64; 4];
    limbs[1] = 1u64 << 32;
    U256::from_limbs(limbs)
};

/// Convert a plain effective `K` (no fractional part) into the Q20.12
/// representation that [`PoolParams::concentration_k`] expects on the wire.
#[inline]
pub fn plain_to_q12_concentration_k(k: u32) -> u32 {
    k.checked_shl(12).unwrap_or(u32::MAX)
}

/// Convert a Q20.12 stored `concentration_k` back to its effective integer
/// `K` (truncated).
#[inline]
pub fn q12_to_plain_concentration_k(k_q12: u32) -> u32 {
    k_q12 >> 12
}

/// Convert a plain decimal price into a Q64.96 sqrt-price (`uint160`) as `u128`.
/// Lossy beyond f64's 53-bit significand. Panics if `price` is negative, NaN,
/// or infinite; saturates at `u128::MAX` on overflow in this adapter.
#[inline]
pub fn price_to_sqrt_price_x96(price: f64) -> u128 {
    assert!(
        price.is_finite() && price >= 0.0,
        "price must be finite and non-negative"
    );
    let scaled = price.sqrt() * 2f64.powi(96);
    if !scaled.is_finite() || scaled < 1.0 {
        return 0;
    }
    if scaled >= u128::MAX as f64 {
        return u128::MAX;
    }
    scaled as u128
}

/// Convert a Q64.96 sqrt-price back to a plain decimal price.
#[inline]
pub fn sqrt_price_x96_to_price(p_x96: u128) -> f64 {
    let sqrt_p = (p_x96 as f64) / 2f64.powi(96);
    sqrt_p * sqrt_p
}

/// Snapshot of pool state required to compute a quote.
pub struct PoolParams {
    /// Operator-published sqrt-price (Q96, uint160 on-chain).
    pub sqrt_price_x96: u128,
    /// Fee charged on Y→X swaps in Q24 (uint24 on-chain).
    pub fee_ask_x24: u32,
    /// Fee charged on X→Y swaps in Q24 (uint24 on-chain).
    pub fee_bid_x24: u32,
    /// Reserve of token X (uint112 on-chain).
    pub reserve_x: u128,
    /// Reserve of token Y (uint112 on-chain).
    pub reserve_y: u128,
    /// Concentration multiplier in Q20.12 (uint32 on-chain).
    pub concentration_k: u32,
}

/// Result of a quote: amount out (net of fee), post-swap sqrt-price, fee paid.
pub struct QuoteResult {
    /// Output amount, net of fee.
    pub amount_out: U256,
    /// Hypothetical post-swap sqrt-price (Q64.96); not persisted by Pool.
    pub sqrt_price_next: u128,
    /// Fee paid in the output token.
    pub fee: U256,
}

/// Compute concentration C in Q48: `cQ48 = mulDiv(K_Q12, r², Q12)`.
pub fn concentration_q48(
    sqrt_price_x96: u128,
    amount_in: U256,
    reserve_x: u128,
    reserve_y: u128,
    concentration_k: u32,
    x_to_y: bool,
) -> U256 {
    if amount_in.is_zero() || concentration_k == 0 || sqrt_price_x96 == 0 {
        return U256::ZERO;
    }

    let sqrt_price = U256::from_u128(sqrt_price_x96);
    // xWealthInY = mulDiv(mulDiv(reserveX, sqrtPX96, Q96), sqrtPX96, Q96)
    //            = reserveX * P (where P = sqrtP^2). Two cascading `/Q96`
    // steps mirror the on-chain `SwapLib.concentrationQ48` exactly — collapsing
    // to a single `sqrt_price² / Q96` divides by the same scale but rounds once,
    // which diverges from Solidity on edge cases.
    let x_wealth_in_y = U256::mul_div(
        U256::mul_div(U256::from_u128(reserve_x), sqrt_price, Q96_U256),
        sqrt_price,
        Q96_U256,
    );
    let total_wealth_in_y = x_wealth_in_y.wrapping_add(U256::from_u128(reserve_y));
    if total_wealth_in_y.is_zero() {
        return U256::ZERO;
    }

    let amount_in_wealth = if x_to_y {
        U256::mul_div(
            U256::mul_div(amount_in, sqrt_price, Q96_U256),
            sqrt_price,
            Q96_U256,
        )
    } else {
        amount_in
    };

    let r_q48 = if amount_in_wealth >= total_wealth_in_y {
        Q48_U256
    } else {
        U256::mul_div(amount_in_wealth, Q48_U256, total_wealth_in_y)
    };

    let r_squared_q48 = U256::mul_div(r_q48, r_q48, Q48_U256);
    let c = U256::mul_div(
        U256::from_u128(concentration_k as u128),
        r_squared_q48,
        Q12_U256,
    );

    if c >= Q48_U256 {
        Q48_U256
    } else {
        c
    }
}

fn lower_bound(sqrt_price_x96: u128, concentration_q48: u64) -> u128 {
    let one_minus_c_q48 = U256::from_u128(Q48 - (concentration_q48 as u128));
    let sqrt_one_minus_c = one_minus_c_q48.isqrt();
    U256::mul_div(
        U256::from_u128(sqrt_price_x96),
        sqrt_one_minus_c,
        U256::from_u128(Q24),
    )
    .as_u128()
}

fn upper_bound(sqrt_price_x96: u128, concentration_q48: u64) -> u128 {
    let one_minus_c_q48 = U256::from_u128(Q48 - (concentration_q48 as u128));
    let sqrt_one_minus_c = one_minus_c_q48.isqrt();
    U256::mul_div(
        U256::from_u128(sqrt_price_x96),
        U256::from_u128(Q24),
        sqrt_one_minus_c,
    )
    .as_u128()
}

fn ly(sqrt_price_x96: u128, p_bid: u128, reserve_y: u128) -> u128 {
    U256::mul_div(
        U256::from_u128(reserve_y),
        Q96_U256,
        U256::from_u128(sqrt_price_x96 - p_bid),
    )
    .as_u128()
}

fn lx(sqrt_price_x96: u128, p_ask: u128, reserve_x: u128) -> u128 {
    let price_product_x96 = U256::mul_div(
        U256::from_u128(sqrt_price_x96),
        U256::from_u128(p_ask),
        Q96_U256,
    );
    U256::mul_div(
        U256::from_u128(reserve_x),
        price_product_x96,
        U256::from_u128(p_ask - sqrt_price_x96),
    )
    .as_u128()
}

/// Mirror of Solidity `SwapLib.applyFee`.
///
/// `feeMultiplier <= 1` collapses to the simple base-fee form.
/// Overflow (`baseFee * feeMultiplier > U256::MAX`) and full-consumption
/// (`scaledFee >= grossOutput`) both yield `(amount_out=0, fee=grossOutput)` —
/// signalling rejection back to callers that compare on `amount_out.is_zero()`.
fn apply_fee(gross_output: U256, fee_q24: u32, fee_multiplier: U256) -> (U256, U256) {
    let base_fee = U256::mul_div(gross_output, U256::from_u128(fee_q24 as u128), Q24_U256);
    let one = U256::from(1u64);
    if fee_multiplier <= one || base_fee.is_zero() {
        return (gross_output - base_fee, base_fee);
    }

    if fee_multiplier > U256::MAX / base_fee {
        return (U256::ZERO, gross_output);
    }

    let scaled_fee = base_fee * fee_multiplier;
    if scaled_fee >= gross_output {
        return (U256::ZERO, gross_output);
    }

    (gross_output - scaled_fee, scaled_fee)
}

fn linear_x_to_y(
    sqrt_price_x96: u128,
    fee_bid_x24: u32,
    reserve_y: u128,
    dx: U256,
    fee_multiplier: U256,
) -> QuoteResult {
    let zero = QuoteResult {
        amount_out: U256::ZERO,
        sqrt_price_next: sqrt_price_x96,
        fee: U256::ZERO,
    };

    let sqrt_price = U256::from_u128(sqrt_price_x96);
    // dy = mulDiv(mulDiv(dx, sqrtPX96, Q96), sqrtPX96, Q96). Cascaded to match
    // on-chain `SwapLib._linearXToY` rounding.
    let dy_gross = U256::mul_div(
        U256::mul_div(dx, sqrt_price, Q96_U256),
        sqrt_price,
        Q96_U256,
    );
    if dy_gross.is_zero() || dy_gross > U256::from_u128(reserve_y) {
        return zero;
    }

    let (amount_out, fee) = apply_fee(dy_gross, fee_bid_x24, fee_multiplier);
    QuoteResult {
        amount_out,
        sqrt_price_next: sqrt_price_x96,
        fee,
    }
}

fn linear_y_to_x(
    sqrt_price_x96: u128,
    fee_ask_x24: u32,
    reserve_x: u128,
    dy: U256,
    fee_multiplier: U256,
) -> QuoteResult {
    let zero = QuoteResult {
        amount_out: U256::ZERO,
        sqrt_price_next: sqrt_price_x96,
        fee: U256::ZERO,
    };

    if sqrt_price_x96 == 0 {
        return zero;
    }
    let sqrt_price = U256::from_u128(sqrt_price_x96);

    // dx = mulDiv(mulDiv(dy, Q96, sqrtPX96), Q96, sqrtPX96). Cascaded to match
    // on-chain `SwapLib._linearYToX` rounding.
    let dx_gross = U256::mul_div(
        U256::mul_div(dy, Q96_U256, sqrt_price),
        Q96_U256,
        sqrt_price,
    );
    if dx_gross.is_zero() || dx_gross > U256::from_u128(reserve_x) {
        return zero;
    }

    let (amount_out, fee) = apply_fee(dx_gross, fee_ask_x24, fee_multiplier);
    QuoteResult {
        amount_out,
        sqrt_price_next: sqrt_price_x96,
        fee,
    }
}

/// Quote a token-X-in / token-Y-out swap with the base bid fee
/// (whitelisted-swapper path; `feeMultiplier == 1`).
pub fn quote_x_to_y(params: &PoolParams, dx: U256) -> QuoteResult {
    quote_x_to_y_with_multiplier(params, dx, U256::from(1u64))
}

/// Quote a token-Y-in / token-X-out swap with the base ask fee.
pub fn quote_y_to_x(params: &PoolParams, dy: U256) -> QuoteResult {
    quote_y_to_x_with_multiplier(params, dy, U256::from(1u64))
}

/// Quote X→Y with an explicit `fee_multiplier`. Mirrors
/// `SwapLib._quoteXToY(..., feeMultiplier)`.
pub fn quote_x_to_y_with_multiplier(
    params: &PoolParams,
    dx: U256,
    fee_multiplier: U256,
) -> QuoteResult {
    let zero = QuoteResult {
        amount_out: U256::ZERO,
        sqrt_price_next: params.sqrt_price_x96,
        fee: U256::ZERO,
    };

    let c_q48 = concentration_q48(
        params.sqrt_price_x96,
        dx,
        params.reserve_x,
        params.reserve_y,
        params.concentration_k,
        true,
    );

    if c_q48.is_zero() {
        return linear_x_to_y(
            params.sqrt_price_x96,
            params.fee_bid_x24,
            params.reserve_y,
            dx,
            fee_multiplier,
        );
    }
    if c_q48 >= Q48_U256 {
        return zero;
    }

    let c_u64 = c_q48.as_u128() as u64;
    let p_bid = lower_bound(params.sqrt_price_x96, c_u64);
    if params.sqrt_price_x96 <= p_bid {
        return zero;
    }
    let liquidity = ly(params.sqrt_price_x96, p_bid, params.reserve_y);

    let max_net_dx = get_amount_x_delta(p_bid, params.sqrt_price_x96, liquidity, false);
    if dx > max_net_dx {
        return zero;
    }

    let p_next =
        get_next_sqrt_price_from_amount_x_rounding_up(params.sqrt_price_x96, liquidity, dx);
    let dy = get_amount_y_delta(params.sqrt_price_x96, p_next, liquidity, false);

    let (amount_out, fee) = apply_fee(dy, params.fee_bid_x24, fee_multiplier);
    QuoteResult {
        amount_out,
        sqrt_price_next: p_next,
        fee,
    }
}

/// Quote Y→X with an explicit `fee_multiplier`.
pub fn quote_y_to_x_with_multiplier(
    params: &PoolParams,
    dy: U256,
    fee_multiplier: U256,
) -> QuoteResult {
    let zero = QuoteResult {
        amount_out: U256::ZERO,
        sqrt_price_next: params.sqrt_price_x96,
        fee: U256::ZERO,
    };

    let c_q48 = concentration_q48(
        params.sqrt_price_x96,
        dy,
        params.reserve_x,
        params.reserve_y,
        params.concentration_k,
        false,
    );

    if c_q48.is_zero() {
        return linear_y_to_x(
            params.sqrt_price_x96,
            params.fee_ask_x24,
            params.reserve_x,
            dy,
            fee_multiplier,
        );
    }
    if c_q48 >= Q48_U256 {
        return zero;
    }

    let c_u64 = c_q48.as_u128() as u64;
    let p_ask = upper_bound(params.sqrt_price_x96, c_u64);
    if params.sqrt_price_x96 >= p_ask {
        return zero;
    }
    let liquidity = lx(params.sqrt_price_x96, p_ask, params.reserve_x);

    let max_net_dy = get_amount_y_delta(params.sqrt_price_x96, p_ask, liquidity, false);
    if dy > max_net_dy {
        return zero;
    }

    let p_next =
        get_next_sqrt_price_from_amount_y_rounding_down(params.sqrt_price_x96, liquidity, dy);
    let dx = get_amount_x_delta(params.sqrt_price_x96, p_next, liquidity, false);

    let (amount_out, fee) = apply_fee(dx, params.fee_ask_x24, fee_multiplier);
    QuoteResult {
        amount_out,
        sqrt_price_next: p_next,
        fee,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concentration_zero_when_inputs_zero() {
        let c = concentration_q48(1u128 << 96, U256::ZERO, 10_000, 10_000, 5_000, true);
        assert_eq!(c, U256::ZERO);

        let c = concentration_q48(1u128 << 96, U256::from(1_000u64), 10_000, 10_000, 0, true);
        assert_eq!(c, U256::ZERO);

        let c = concentration_q48(0, U256::from(1_000u64), 10_000, 10_000, 5_000, true);
        assert_eq!(c, U256::ZERO);
    }

    #[test]
    fn quote_x_to_y_uses_linear_when_k_zero() {
        let params = PoolParams {
            sqrt_price_x96: 1u128 << 96,
            fee_ask_x24: 0,
            fee_bid_x24: ((5u32) * (1u32 << 24)) / 100,
            reserve_x: 1_000_000,
            reserve_y: 1_000_000,
            concentration_k: 0,
        };
        let result = quote_x_to_y(&params, U256::from(1_000u64));
        assert_eq!(result.fee, U256::from(49u64));
        assert_eq!(result.amount_out, U256::from(951u64));
        assert_eq!(result.sqrt_price_next, params.sqrt_price_x96);
    }

    #[test]
    fn fee_multiplier_doubles_fee() {
        let params = PoolParams {
            sqrt_price_x96: 1u128 << 96,
            fee_ask_x24: 0,
            fee_bid_x24: ((5u32) * (1u32 << 24)) / 100,
            reserve_x: 1_000_000,
            reserve_y: 1_000_000,
            concentration_k: 0,
        };
        let base = quote_x_to_y(&params, U256::from(1_000u64));
        let scaled = quote_x_to_y_with_multiplier(&params, U256::from(1_000u64), U256::from(2u64));
        assert_eq!(scaled.fee, base.fee * U256::from(2u64));
        assert_eq!(scaled.amount_out, U256::from(1_000u64) - scaled.fee);
    }

    #[test]
    fn fee_multiplier_consumes_all_output_when_too_large() {
        let params = PoolParams {
            sqrt_price_x96: 1u128 << 96,
            fee_ask_x24: 0,
            fee_bid_x24: ((5u32) * (1u32 << 24)) / 100,
            reserve_x: 1_000_000,
            reserve_y: 1_000_000,
            concentration_k: 0,
        };
        let result =
            quote_x_to_y_with_multiplier(&params, U256::from(1_000u64), U256::from(100u64));
        assert_eq!(result.amount_out, U256::ZERO);
        assert_eq!(result.fee, U256::from(1_000u64));
    }

    #[test]
    fn quote_returns_zero_when_no_liquidity() {
        let params = PoolParams {
            sqrt_price_x96: 1u128 << 96,
            fee_ask_x24: 0,
            fee_bid_x24: 0,
            reserve_x: 0,
            reserve_y: 0,
            concentration_k: 5_000,
        };
        let result = quote_x_to_y(&params, U256::from(1_000u64));
        assert_eq!(result.amount_out, U256::ZERO);
    }
}
