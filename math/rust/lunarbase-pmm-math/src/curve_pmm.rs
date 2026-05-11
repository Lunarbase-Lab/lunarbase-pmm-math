//! Direct port of the Solidity `SwapLib` quoting library on the
//! `fix/incident` branch (single-price Q64.96 design).
//!
//! Key properties:
//!
//! * Single sqrt-price `sqrt_price_x96` (Q64.96, uint160 on-chain). There is
//!   no separate live-vs-anchor price; the operator-published anchor *is* the
//!   only price. Swaps compute a hypothetical `pNext` but do not write back
//!   into state — only `upd()` changes the price. This eliminates the
//!   drift-based round-trip exploit by construction.
//! * Asymmetric directional fees `fee_bid_x24` (X→Y) and `fee_ask_x24` (Y→X),
//!   both in Q24.
//! * `concentration_k` is Q20.12; effective K = `concentration_k / 2^12`.
//! * Concentration formula: `cQ48 = mulDiv(K_Q12, r²_Q48, Q12)` with `r`
//!   wealth-normalised by `sqrtPriceX96²`.
//! * Linear-fallback path when `cQ48 == 0`: `dy = mulDiv(mulDiv(dx, p, Q96), p, Q96)`,
//!   fee on output; `pNext = sqrt_price_x96`.

use crate::sqrt_price_math::{
    get_amount_x_delta, get_amount_y_delta, get_next_sqrt_price_from_amount_x_rounding_up,
    get_next_sqrt_price_from_amount_y_rounding_down,
};
use crate::uint256::{U256Ext, U256};

/// Q48 fixed-point unit (`2^48`), used by the concentration value `cQ48`.
pub const Q48: u128 = 1u128 << 48;
/// Q24 fixed-point unit (`2^24`), used by directional fees and as the
/// scaling factor for `lower_bound`/`upper_bound`.
pub const Q24: u128 = 1u128 << 24;
/// Q12 fixed-point unit (`2^12`), used as the denominator when scaling the
/// stored `concentration_k` to its effective value.
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
const Q96_U256: U256 = U256::Q96;

/// Lift a Q32.48 sqrt-price (legacy `pX48`, uint80) into a Q64.96 sqrt-price
/// (`pX96`, uint160) by shifting left 48 bits. The result represents the
/// same numerical price (same value of `(p/Q)²`).
#[inline]
pub fn sqrt_price_x48_to_x96(p_x48: u128) -> U256 {
    U256::from_u128(p_x48).shl(48)
}

/// Lower a Q64.96 sqrt-price (`pX96`, uint160) into a Q32.48 sqrt-price
/// (`pX48`, uint80) by right-shifting 48 bits, truncating the bottom 48 bits
/// of precision. Used for backward-compat with legacy serialised state.
#[inline]
pub fn sqrt_price_x96_to_x48(p_x96: U256) -> u128 {
    let shifted = p_x96.shr(48);
    debug_assert!(shifted.fits_u128(), "pX96 >> 48 overflows u128 (≥2^128)");
    shifted.as_u128()
}

/// Snapshot of pool state required to compute a quote.
pub struct PoolParams {
    /// Sqrt-price in Q64.96 (uint160 on-chain). Only operator's `upd()`
    /// changes this — swaps do not mutate it.
    pub sqrt_price_x96: U256,
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

/// Result of a quote: amount out (net of fee), post-swap sqrt-price, and fee paid.
pub struct QuoteResult {
    /// Output amount, net of [`Self::fee`].
    pub amount_out: U256,
    /// Hypothetical sqrt-price the swap would move toward. Returned as
    /// information; the pool's stored `sqrt_price_x96` is unchanged.
    pub sqrt_price_next: U256,
    /// Fee paid in the output token.
    pub fee: U256,
}

/// Concentration `cQ48 = mulDiv(K_Q12, r²_Q48, Q12)`. `r` is wealth-normalised
/// using `sqrtPriceX96²`. Saturates at `Q48` (100%).
pub fn concentration_q48(
    sqrt_price_x96: U256,
    amount_in: U256,
    reserve_x: u128,
    reserve_y: u128,
    concentration_k: u32,
    x_to_y: bool,
) -> U256 {
    if amount_in.is_zero() || concentration_k == 0 || sqrt_price_x96.is_zero() {
        return U256::ZERO;
    }

    // wealthX_in_Y = mulDiv(mulDiv(reserveX, sqrtPX96, Q96), sqrtPX96, Q96)
    let x_wealth_in_y = U256::mul_div(
        U256::mul_div(U256::from_u128(reserve_x), sqrt_price_x96, Q96_U256),
        sqrt_price_x96,
        Q96_U256,
    );
    let total_wealth_in_y = x_wealth_in_y.wrapping_add(U256::from_u128(reserve_y));
    if total_wealth_in_y.is_zero() {
        return U256::ZERO;
    }

    let amount_in_wealth = if x_to_y {
        U256::mul_div(
            U256::mul_div(amount_in, sqrt_price_x96, Q96_U256),
            sqrt_price_x96,
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

fn lower_bound(sqrt_price_x96: U256, concentration_q48: u64) -> U256 {
    let one_minus_c_q48 = U256::from_u128(Q48 - (concentration_q48 as u128));
    let sqrt_one_minus_c = one_minus_c_q48.isqrt();

    U256::mul_div(sqrt_price_x96, sqrt_one_minus_c, U256::from_u128(Q24))
}

fn upper_bound(sqrt_price_x96: U256, concentration_q48: u64) -> U256 {
    let one_minus_c_q48 = U256::from_u128(Q48 - (concentration_q48 as u128));
    let sqrt_one_minus_c = one_minus_c_q48.isqrt();

    U256::mul_div(sqrt_price_x96, U256::from_u128(Q24), sqrt_one_minus_c)
}

fn ly(sqrt_price_x96: U256, p_bid: U256, reserve_y: u128) -> u128 {
    // L_y = mulDiv(yReserve, Q96, pX96 - pBid)
    let result = U256::mul_div(U256::from_u128(reserve_y), Q96_U256, sqrt_price_x96 - p_bid);
    assert!(result.fits_u128(), "Ly overflows u128");
    result.as_u128()
}

fn lx(sqrt_price_x96: U256, p_ask: U256, reserve_x: u128) -> u128 {
    // priceProductX96 = mulDiv(sqrt_price_x96, p_ask, Q96)
    // L_x = mulDiv(xReserve, priceProductX96, p_ask - sqrt_price_x96)
    let price_product_x96 = U256::mul_div(sqrt_price_x96, p_ask, Q96_U256);
    let result = U256::mul_div(
        U256::from_u128(reserve_x),
        price_product_x96,
        p_ask - sqrt_price_x96,
    );
    assert!(result.fits_u128(), "Lx overflows u128");
    result.as_u128()
}

fn apply_fee(gross: U256, fee_x24: u32) -> (U256, U256) {
    let fee = U256::mul_div(gross, U256::from_u128(fee_x24 as u128), Q24_U256);
    (gross - fee, fee)
}

fn linear_x_to_y(sqrt_price_x96: U256, fee_bid_x24: u32, reserve_y: u128, dx: U256) -> QuoteResult {
    let zero = QuoteResult {
        amount_out: U256::ZERO,
        sqrt_price_next: sqrt_price_x96,
        fee: U256::ZERO,
    };

    // dy = mulDiv(mulDiv(dx, p, Q96), p, Q96)
    let dy = U256::mul_div(
        U256::mul_div(dx, sqrt_price_x96, Q96_U256),
        sqrt_price_x96,
        Q96_U256,
    );
    if dy.is_zero() || dy > U256::from_u128(reserve_y) {
        return zero;
    }

    let (amount_out, fee) = apply_fee(dy, fee_bid_x24);
    QuoteResult {
        amount_out,
        sqrt_price_next: sqrt_price_x96,
        fee,
    }
}

fn linear_y_to_x(sqrt_price_x96: U256, fee_ask_x24: u32, reserve_x: u128, dy: U256) -> QuoteResult {
    let zero = QuoteResult {
        amount_out: U256::ZERO,
        sqrt_price_next: sqrt_price_x96,
        fee: U256::ZERO,
    };

    if sqrt_price_x96.is_zero() {
        return zero;
    }

    // dx = mulDiv(mulDiv(dy, Q96, p), Q96, p)
    let dx = U256::mul_div(
        U256::mul_div(dy, Q96_U256, sqrt_price_x96),
        Q96_U256,
        sqrt_price_x96,
    );
    if dx.is_zero() || dx > U256::from_u128(reserve_x) {
        return zero;
    }

    let (amount_out, fee) = apply_fee(dx, fee_ask_x24);
    QuoteResult {
        amount_out,
        sqrt_price_next: sqrt_price_x96,
        fee,
    }
}

/// Quote a token-X-in / token-Y-out swap. Mirrors Solidity `SwapLib._quoteXToY`
/// on the `fix/incident` branch bit-for-bit.
pub fn quote_x_to_y(params: &PoolParams, dx: U256) -> QuoteResult {
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
    let (amount_out, fee) = apply_fee(dy, params.fee_bid_x24);
    QuoteResult {
        amount_out,
        sqrt_price_next: p_next,
        fee,
    }
}

/// Quote a token-Y-in / token-X-out swap. Mirrors Solidity `SwapLib._quoteYToX`
/// on the `fix/incident` branch bit-for-bit.
pub fn quote_y_to_x(params: &PoolParams, dy: U256) -> QuoteResult {
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
    let (amount_out, fee) = apply_fee(dx, params.fee_ask_x24);
    QuoteResult {
        amount_out,
        sqrt_price_next: p_next,
        fee,
    }
}
