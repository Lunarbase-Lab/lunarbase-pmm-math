//! Direct port of the Solidity `CurvePMM` quoting library.

use crate::sqrt_price_math::{
    get_amount_x_delta, get_amount_y_delta, get_next_sqrt_price_from_amount_x_rounding_up,
    get_next_sqrt_price_from_amount_y_rounding_down,
};
use crate::uint256::{U256Ext, U256};

const Q48: u128 = 1u128 << 48;
const Q24: u128 = 1u128 << 24;

// Q48 / Q96 as `U256` constants. Pre-computed at compile time so `concentration_q48`
// doesn't pay for `from_u128` + `wrapping_mul` on every quote.
const Q48_U256: U256 = U256::Q48;
const Q96_U256: U256 = {
    let mut limbs = [0u64; 4];
    limbs[1] = 1u64 << 32; // 2^96 = (2^64) * 2^32 → goes into the second limb
    U256::from_limbs(limbs)
};

/// Snapshot of pool state required to compute a quote.
///
/// All fixed-point values use Q48. `sqrt_price_x48` and
/// `anchor_sqrt_price_x48` are uint80, `fee_q48` is uint48, `reserve_x` and
/// `reserve_y` are uint112; the wider Rust types here are arithmetic-only.
pub struct PoolParams {
    /// Current sqrt-price (Q48, uint80).
    pub sqrt_price_x48: u128,
    /// Operator-published anchor sqrt-price (Q48, uint80).
    pub anchor_sqrt_price_x48: u128,
    /// Base fee (Q48, uint48).
    pub fee_q48: u64,
    /// Reserve of token X (uint112).
    pub reserve_x: u128,
    /// Reserve of token Y (uint112).
    pub reserve_y: u128,
    /// Concentration multiplier `k`.
    pub concentration_k: u32,
}

/// Result of a quote: amount out (net of fee), post-swap sqrt-price, and fee paid.
pub struct QuoteResult {
    /// Output amount, net of [`Self::fee`].
    pub amount_out: U256,
    /// Sqrt-price after applying the swap.
    pub sqrt_price_next: u128,
    /// Fee paid in the output token.
    pub fee: U256,
}

/// Compute concentration C in Q48: C = fee * (1 + k * r^2).
/// r is normalized by wealth, not raw input reserve.
fn concentration_q48(
    sqrt_price_x48: u128,
    base_fee_q48: u64,
    amount_in: U256,
    reserve_x: u128,
    reserve_y: u128,
    k: u32,
    x_to_y: bool,
) -> U256 {
    let c = U256::from_u128(base_fee_q48 as u128);

    if c.is_zero() || amount_in.is_zero() || k == 0 || sqrt_price_x48 == 0 {
        return c;
    }

    let sqrt_price = U256::from_u128(sqrt_price_x48);
    let price_q96 = sqrt_price.wrapping_mul(sqrt_price);
    let x_wealth_in_y = U256::mul_div(U256::from_u128(reserve_x), price_q96, Q96_U256);
    let total_wealth_in_y = x_wealth_in_y.wrapping_add(U256::from_u128(reserve_y));
    if total_wealth_in_y.is_zero() {
        return c;
    }

    let amount_in_wealth = if x_to_y {
        U256::mul_div(amount_in, price_q96, Q96_U256)
    } else {
        amount_in
    };

    // r in Q48: min(amountInWealth/totalWealth, 1) * Q48
    let r_q48 = if amount_in_wealth >= total_wealth_in_y {
        Q48_U256
    } else {
        U256::mul_div(amount_in_wealth, Q48_U256, total_wealth_in_y)
    };

    // r^2 in Q48
    let r_squared_q48 = U256::mul_div(r_q48, r_q48, Q48_U256);

    // multiplier = Q48 + k * r^2
    let multiplier_q48 =
        Q48_U256.wrapping_add(U256::from_u128(k as u128).wrapping_mul(r_squared_q48));

    // C = fee * multiplier / Q48
    let result = U256::mul_div(c, multiplier_q48, Q48_U256);

    if result >= Q48_U256 {
        Q48_U256
    } else {
        result
    }
}

fn lower_bound(sqrt_price_x48: u128, concentration_q48: u64) -> u128 {
    let one_minus_c_q48 = U256::from_u128(Q48 - (concentration_q48 as u128));
    let sqrt_one_minus_c = one_minus_c_q48.isqrt();

    U256::mul_div(
        U256::from_u128(sqrt_price_x48),
        sqrt_one_minus_c,
        U256::from_u128(Q24),
    )
    .as_u128()
}

fn upper_bound(sqrt_price_x48: u128, concentration_q48: u64) -> u128 {
    let one_minus_c_q48 = U256::from_u128(Q48 - (concentration_q48 as u128));
    let sqrt_one_minus_c = one_minus_c_q48.isqrt();

    U256::mul_div(
        U256::from_u128(sqrt_price_x48),
        U256::from_u128(Q24),
        sqrt_one_minus_c,
    )
    .as_u128()
}

fn ly(sqrt_price_x48: u128, p_bid: u128, reserve_y: u128) -> u128 {
    U256::mul_div(
        U256::from_u128(reserve_y),
        Q48_U256,
        U256::from_u128(sqrt_price_x48 - p_bid),
    )
    .as_u128()
}

fn lx(sqrt_price_x48: u128, p_ask: u128, reserve_x: u128) -> u128 {
    U256::mul_div(
        U256::from_u128(reserve_x),
        U256::from_u128(sqrt_price_x48).wrapping_mul(U256::from_u128(p_ask)),
        Q48_U256.wrapping_mul(U256::from_u128(p_ask - sqrt_price_x48)),
    )
    .as_u128()
}

/// Quote a token-X-in / token-Y-out swap.
///
/// Exact port of `CurvePMM.quoteXToY`. Returns a [`QuoteResult`] with zero
/// `amount_out` and unchanged `sqrt_price_next` when the swap is rejected.
pub fn quote_x_to_y(params: &PoolParams, dx: U256) -> QuoteResult {
    let zero = QuoteResult {
        amount_out: U256::ZERO,
        sqrt_price_next: params.sqrt_price_x48,
        fee: U256::ZERO,
    };

    let c_q48 = concentration_q48(
        params.sqrt_price_x48,
        params.fee_q48,
        dx,
        params.reserve_x,
        params.reserve_y,
        params.concentration_k,
        true,
    );

    if c_q48 >= Q48_U256 {
        return zero;
    }

    let c_u64 = c_q48.as_u128() as u64;
    let p_bid = lower_bound(params.anchor_sqrt_price_x48, c_u64);
    if params.sqrt_price_x48 <= p_bid {
        return zero;
    }
    let liquidity = ly(params.sqrt_price_x48, p_bid, params.reserve_y);

    // maxNetDx = getAmountXDelta(pBid, pX48, liquidity, false)
    let max_net_dx = get_amount_x_delta(p_bid, params.sqrt_price_x48, liquidity, false);

    if dx > max_net_dx {
        return zero;
    }

    // pNext = getNextSqrtPriceFromInput(pX48, liquidity, dx, true)
    let p_next =
        get_next_sqrt_price_from_amount_x_rounding_up(params.sqrt_price_x48, liquidity, dx);

    // dy = getAmountYDelta(pX48, pNext, liquidity, false)
    let dy = get_amount_y_delta(params.sqrt_price_x48, p_next, liquidity, false);

    // fee = dy * feeQ48 / Q48
    let fee = U256::mul_div(dy, U256::from_u128(params.fee_q48 as u128), Q48_U256);
    let dy_after_fee = dy - fee;

    QuoteResult {
        amount_out: dy_after_fee,
        sqrt_price_next: p_next,
        fee,
    }
}

/// Quote a token-Y-in / token-X-out swap.
///
/// Exact port of `CurvePMM.quoteYToX`. Returns a [`QuoteResult`] with zero
/// `amount_out` and unchanged `sqrt_price_next` when the swap is rejected.
pub fn quote_y_to_x(params: &PoolParams, dy: U256) -> QuoteResult {
    let zero = QuoteResult {
        amount_out: U256::ZERO,
        sqrt_price_next: params.sqrt_price_x48,
        fee: U256::ZERO,
    };

    let c_q48 = concentration_q48(
        params.sqrt_price_x48,
        params.fee_q48,
        dy,
        params.reserve_x,
        params.reserve_y,
        params.concentration_k,
        false,
    );

    if c_q48 >= Q48_U256 {
        return zero;
    }

    let c_u64 = c_q48.as_u128() as u64;
    let p_ask = upper_bound(params.anchor_sqrt_price_x48, c_u64);
    if params.sqrt_price_x48 >= p_ask {
        return zero;
    }
    let liquidity = lx(params.sqrt_price_x48, p_ask, params.reserve_x);

    // maxNetDy = getAmountYDelta(pX48, pAsk, liquidity, false)
    let max_net_dy = get_amount_y_delta(params.sqrt_price_x48, p_ask, liquidity, false);

    if dy > max_net_dy {
        return zero;
    }

    // pNext = getNextSqrtPriceFromInput(pX48, liquidity, dy, false)
    let p_next =
        get_next_sqrt_price_from_amount_y_rounding_down(params.sqrt_price_x48, liquidity, dy);

    // dx = getAmountXDelta(pX48, pNext, liquidity, false)
    let dx = get_amount_x_delta(params.sqrt_price_x48, p_next, liquidity, false);

    // fee = dx * feeQ48 / Q48
    let fee = U256::mul_div(dx, U256::from_u128(params.fee_q48 as u128), Q48_U256);
    let dx_after_fee = dx - fee;

    QuoteResult {
        amount_out: dx_after_fee,
        sqrt_price_next: p_next,
        fee,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_concentration_zero_fee() {
        let c = concentration_q48(
            1u128 << 48,
            0,
            U256::from(1000u64),
            10000,
            10000,
            5000,
            true,
        );
        assert_eq!(c, U256::ZERO);
    }

    #[test]
    fn test_concentration_zero_amount() {
        let c = concentration_q48(1u128 << 48, 1000, U256::ZERO, 10000, 10000, 5000, true);
        assert_eq!(c, U256::from(1000u64));
    }

    #[test]
    fn test_quote_returns_zero_when_no_liquidity() {
        let params = PoolParams {
            sqrt_price_x48: 1u128 << 48,
            anchor_sqrt_price_x48: 1u128 << 48,
            fee_q48: 1u64 << 44,
            reserve_x: 0,
            reserve_y: 0,
            concentration_k: 5000,
        };
        let result = quote_x_to_y(&params, U256::from(1000u64));
        assert_eq!(result.amount_out, U256::ZERO);
    }
}
