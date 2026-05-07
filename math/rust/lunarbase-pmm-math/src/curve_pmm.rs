//! Direct port of the Solidity `SwapLib` quoting library on
//! `update/asymetric-fees`.
//!
//! Key differences from the previous (`dev`) revision:
//!
//! * Single Q48 `fee` is replaced by directional Q24 fees: `fee_bid_x24`
//!   (charged on X→Y) and `fee_ask_x24` (charged on Y→X).
//! * `concentration_k` is now Q20.12 (`concentration_k_q12`); effective `K`
//!   = `concentration_k_q12 / 2^12`.
//! * Concentration is no longer mixed with the base fee or biased by `+1`.
//!   Pure form: `cQ48 = mulDiv(concentrationKQ12, r², Q12)` with wealth
//!   normalised by `anchorPrice` (not the live sqrt-price).
//! * When `cQ48 == 0` the swap takes a linear-fallback path using the
//!   anchor price as a flat constant; `pNext` is left at `pX48`.

use crate::sqrt_price_math::{
    get_amount_x_delta, get_amount_y_delta, get_next_sqrt_price_from_amount_x_rounding_up,
    get_next_sqrt_price_from_amount_y_rounding_down,
};
use crate::uint256::{U256Ext, U256};

/// Q48 fixed-point unit (`2^48`), used by `pX48`, sqrt-price math, and the
/// concentration value `cQ48`.
pub const Q48: u128 = 1u128 << 48;
/// Q24 fixed-point unit (`2^24`), used by directional fees and as the
/// scaling factor for `lower_bound`/`upper_bound`.
pub const Q24: u128 = 1u128 << 24;
/// Q12 fixed-point unit (`2^12`), used as the denominator when scaling the
/// stored `concentration_k_q12` to its effective value.
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
    limbs[1] = 1u64 << 32; // 2^96 = 2^64 * 2^32
    U256::from_limbs(limbs)
};

/// Snapshot of pool state required to compute a quote.
///
/// Values are stored at the widths used by the on-chain contract; the wider
/// Rust types here are arithmetic-only.
pub struct PoolParams {
    /// Live swap-driven sqrt-price (Q48, uint80 on-chain).
    pub sqrt_price_x48: u128,
    /// Operator-published anchor sqrt-price (Q48, uint80 on-chain).
    pub anchor_sqrt_price_x48: u128,
    /// Fee charged on Y→X swaps in Q24 (uint24 on-chain).
    pub fee_ask_x24: u32,
    /// Fee charged on X→Y swaps in Q24 (uint24 on-chain).
    pub fee_bid_x24: u32,
    /// Reserve of token X (uint112 on-chain).
    pub reserve_x: u128,
    /// Reserve of token Y (uint112 on-chain).
    pub reserve_y: u128,
    /// Concentration multiplier in Q20.12 (uint32 on-chain). Effective
    /// `K = concentration_k_q12 / 2^12`.
    pub concentration_k_q12: u32,
}

/// Result of a quote: amount out (net of fee), post-swap sqrt-price, and fee paid.
pub struct QuoteResult {
    /// Output amount, net of [`Self::fee`].
    pub amount_out: U256,
    /// Sqrt-price after applying the swap. Equals `sqrt_price_x48` when the
    /// swap is rejected or routed through the linear-fallback path.
    pub sqrt_price_next: u128,
    /// Fee paid in the output token.
    pub fee: U256,
}

/// Compute concentration C in Q48 from the asymmetric-fees branch:
/// `cQ48 = mulDiv(concentrationKQ12, rSquaredQ48, Q12)`, where
/// `r = min(amountInWealth / totalWealth, 1)` is normalised by anchor-price
/// wealth (not raw input reserve). Saturates at Q48 (100%).
pub fn concentration_q48(
    anchor_sqrt_price_x48: u128,
    amount_in: U256,
    reserve_x: u128,
    reserve_y: u128,
    concentration_k_q12: u32,
    x_to_y: bool,
) -> U256 {
    if amount_in.is_zero() || concentration_k_q12 == 0 || anchor_sqrt_price_x48 == 0 {
        return U256::ZERO;
    }

    let anchor = U256::from_u128(anchor_sqrt_price_x48);
    let price_q96 = anchor.wrapping_mul(anchor);
    let x_wealth_in_y = U256::mul_div(U256::from_u128(reserve_x), price_q96, Q96_U256);
    let total_wealth_in_y = x_wealth_in_y.wrapping_add(U256::from_u128(reserve_y));
    if total_wealth_in_y.is_zero() {
        return U256::ZERO;
    }

    let amount_in_wealth = if x_to_y {
        U256::mul_div(amount_in, price_q96, Q96_U256)
    } else {
        amount_in
    };

    // r in Q48: min(amountInWealth / totalWealth, 1) * Q48
    let r_q48 = if amount_in_wealth >= total_wealth_in_y {
        Q48_U256
    } else {
        U256::mul_div(amount_in_wealth, Q48_U256, total_wealth_in_y)
    };

    // r² in Q48
    let r_squared_q48 = U256::mul_div(r_q48, r_q48, Q48_U256);

    // c = mulDiv(K_Q12, r²Q48, Q12)  (round-down)
    let c = U256::mul_div(
        U256::from_u128(concentration_k_q12 as u128),
        r_squared_q48,
        Q12_U256,
    );

    if c >= Q48_U256 {
        Q48_U256
    } else {
        c
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

/// Linear-fallback X→Y path: `dy = mulDiv(dx, anchor², Q96)`, fee on `dy`,
/// `pNext = pX48`. Reserve check is performed on `dy` *before* fee, mirroring
/// Solidity ordering bit-for-bit.
fn linear_x_to_y(
    sqrt_price_x48: u128,
    anchor_sqrt_price_x48: u128,
    fee_bid_x24: u32,
    reserve_y: u128,
    dx: U256,
) -> QuoteResult {
    let zero = QuoteResult {
        amount_out: U256::ZERO,
        sqrt_price_next: sqrt_price_x48,
        fee: U256::ZERO,
    };

    let anchor = U256::from_u128(anchor_sqrt_price_x48);
    let price_q96 = anchor.wrapping_mul(anchor);

    let dy_gross = U256::mul_div(dx, price_q96, Q96_U256);
    if dy_gross.is_zero() || dy_gross > U256::from_u128(reserve_y) {
        return zero;
    }

    let fee = U256::mul_div(dy_gross, U256::from_u128(fee_bid_x24 as u128), Q24_U256);
    QuoteResult {
        amount_out: dy_gross - fee,
        sqrt_price_next: sqrt_price_x48,
        fee,
    }
}

/// Linear-fallback Y→X path: `dx = mulDiv(dy, Q96, anchor²)`, fee on `dx`,
/// `pNext = pX48`.
fn linear_y_to_x(
    sqrt_price_x48: u128,
    anchor_sqrt_price_x48: u128,
    fee_ask_x24: u32,
    reserve_x: u128,
    dy: U256,
) -> QuoteResult {
    let zero = QuoteResult {
        amount_out: U256::ZERO,
        sqrt_price_next: sqrt_price_x48,
        fee: U256::ZERO,
    };

    let anchor = U256::from_u128(anchor_sqrt_price_x48);
    let price_q96 = anchor.wrapping_mul(anchor);
    if price_q96.is_zero() {
        return zero;
    }

    let dx_gross = U256::mul_div(dy, Q96_U256, price_q96);
    if dx_gross.is_zero() || dx_gross > U256::from_u128(reserve_x) {
        return zero;
    }

    let fee = U256::mul_div(dx_gross, U256::from_u128(fee_ask_x24 as u128), Q24_U256);
    QuoteResult {
        amount_out: dx_gross - fee,
        sqrt_price_next: sqrt_price_x48,
        fee,
    }
}

/// Quote a token-X-in / token-Y-out swap.
///
/// Exact port of `SwapLib._quoteXToY` on `update/asymetric-fees`. Returns a
/// `QuoteResult` with zero `amount_out` and unchanged `sqrt_price_next` when
/// the swap is rejected.
pub fn quote_x_to_y(params: &PoolParams, dx: U256) -> QuoteResult {
    let zero = QuoteResult {
        amount_out: U256::ZERO,
        sqrt_price_next: params.sqrt_price_x48,
        fee: U256::ZERO,
    };

    let c_q48 = concentration_q48(
        params.anchor_sqrt_price_x48,
        dx,
        params.reserve_x,
        params.reserve_y,
        params.concentration_k_q12,
        true,
    );

    if c_q48.is_zero() {
        return linear_x_to_y(
            params.sqrt_price_x48,
            params.anchor_sqrt_price_x48,
            params.fee_bid_x24,
            params.reserve_y,
            dx,
        );
    }
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

    let p_next =
        get_next_sqrt_price_from_amount_x_rounding_up(params.sqrt_price_x48, liquidity, dx);
    let dy = get_amount_y_delta(params.sqrt_price_x48, p_next, liquidity, false);

    let fee = U256::mul_div(dy, U256::from_u128(params.fee_bid_x24 as u128), Q24_U256);
    QuoteResult {
        amount_out: dy - fee,
        sqrt_price_next: p_next,
        fee,
    }
}

/// Quote a token-Y-in / token-X-out swap.
///
/// Exact port of `SwapLib._quoteYToX` on `update/asymetric-fees`.
pub fn quote_y_to_x(params: &PoolParams, dy: U256) -> QuoteResult {
    let zero = QuoteResult {
        amount_out: U256::ZERO,
        sqrt_price_next: params.sqrt_price_x48,
        fee: U256::ZERO,
    };

    let c_q48 = concentration_q48(
        params.anchor_sqrt_price_x48,
        dy,
        params.reserve_x,
        params.reserve_y,
        params.concentration_k_q12,
        false,
    );

    if c_q48.is_zero() {
        return linear_y_to_x(
            params.sqrt_price_x48,
            params.anchor_sqrt_price_x48,
            params.fee_ask_x24,
            params.reserve_x,
            dy,
        );
    }
    if c_q48 >= Q48_U256 {
        return zero;
    }

    let c_u64 = c_q48.as_u128() as u64;
    let p_ask = upper_bound(params.anchor_sqrt_price_x48, c_u64);
    if params.sqrt_price_x48 >= p_ask {
        return zero;
    }
    let liquidity = lx(params.sqrt_price_x48, p_ask, params.reserve_x);

    let max_net_dy = get_amount_y_delta(params.sqrt_price_x48, p_ask, liquidity, false);
    if dy > max_net_dy {
        return zero;
    }

    let p_next =
        get_next_sqrt_price_from_amount_y_rounding_down(params.sqrt_price_x48, liquidity, dy);
    let dx = get_amount_x_delta(params.sqrt_price_x48, p_next, liquidity, false);

    let fee = U256::mul_div(dx, U256::from_u128(params.fee_ask_x24 as u128), Q24_U256);
    QuoteResult {
        amount_out: dx - fee,
        sqrt_price_next: p_next,
        fee,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concentration_zero_when_inputs_zero() {
        let c = concentration_q48(1u128 << 48, U256::ZERO, 10_000, 10_000, 5_000, true);
        assert_eq!(c, U256::ZERO);

        let c = concentration_q48(1u128 << 48, U256::from(1_000u64), 10_000, 10_000, 0, true);
        assert_eq!(c, U256::ZERO);

        let c = concentration_q48(0, U256::from(1_000u64), 10_000, 10_000, 5_000, true);
        assert_eq!(c, U256::ZERO);
    }

    #[test]
    fn quote_x_to_y_uses_linear_when_k_zero() {
        let params = PoolParams {
            sqrt_price_x48: 1u128 << 48,
            anchor_sqrt_price_x48: 1u128 << 48,
            fee_ask_x24: 0,
            fee_bid_x24: ((5u32) * (1u32 << 24)) / 100, // 5% bid
            reserve_x: 1_000_000,
            reserve_y: 1_000_000,
            concentration_k_q12: 0,
        };
        let result = quote_x_to_y(&params, U256::from(1_000u64));
        // Linear path: dy_gross = floor(1000 * Q48² / Q48²) = 1000.
        // 5% in Q24 truncates to 838_860, so effective fee ≈ 4.99999%; the
        // 1000-wei fee floor-rounds to 49, not 50.
        assert_eq!(result.fee, U256::from(49u64));
        assert_eq!(result.amount_out, U256::from(951u64));
        assert_eq!(result.sqrt_price_next, params.sqrt_price_x48);
    }

    #[test]
    fn quote_returns_zero_when_no_liquidity() {
        let params = PoolParams {
            sqrt_price_x48: 1u128 << 48,
            anchor_sqrt_price_x48: 1u128 << 48,
            fee_ask_x24: 0,
            fee_bid_x24: 0,
            reserve_x: 0,
            reserve_y: 0,
            concentration_k_q12: 5_000,
        };
        let result = quote_x_to_y(&params, U256::from(1_000u64));
        assert_eq!(result.amount_out, U256::ZERO);
    }
}
