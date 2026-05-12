//! Uniswap V3-style sqrt-price arithmetic in Q32.48 fixed-point (uint80).
//!
//! Mirrors the on-chain `SqrtPriceMath` library on the `fix/incident` branch
//! (single-price Q32.48 design) and matches the rounding modes used by
//! `SwapLib._quoteXToY` / `_quoteYToX`.
//!
//! Sqrt-prices are uint80 (≤ 2^80 − 1) and exposed as `u128` for ergonomics.
//! Intermediate products that exceed 128 bits use [`U256`] to avoid overflow.

use crate::uint256::{U256Ext, U256};

const Q48: U256 = U256::Q48;
const U80_MAX: u128 = (1u128 << 80) - 1;

#[inline]
fn assert_fits_u80(v: U256) -> u128 {
    debug_assert!(v.fits_u80(), "sqrt price overflow u80");
    assert!(v.fits_u80(), "sqrt price overflow u80");
    v.as_u128()
}

/// `getNextSqrtPriceFromAmountXRoundingUp` (addX=true), used by `quoteXToY`.
/// Sqrt-prices are uint80 (Q32.48), passed as `u128`. Result asserted to fit
/// uint80.
#[inline]
pub fn get_next_sqrt_price_from_amount_x_rounding_up(
    sqrt_px48: u128,
    liquidity: u128,
    amount_x: U256,
) -> u128 {
    if amount_x.is_zero() {
        return sqrt_px48;
    }

    let sqrt = U256::from_u128(sqrt_px48);
    // numerator1 = liquidity << 48
    let numerator1 = U256::from_u128(liquidity).shl(48);

    // product = amountX * sqrtPX48 (unchecked wrap matching Solidity)
    let product = amount_x.wrapping_mul(sqrt);

    // Overflow-check: product / amountX == sqrtPX48
    if !amount_x.is_zero() && product / amount_x == sqrt {
        let denominator = numerator1.wrapping_add(product);
        if denominator >= numerator1 {
            return assert_fits_u80(U256::mul_div_ceil(numerator1, sqrt, denominator));
        }
    }

    // Fallback: ceilDiv(numerator1, numerator1/sqrtPX48 + amountX)
    let div_result = numerator1 / sqrt;
    let denominator = div_result.wrapping_add(amount_x);
    assert_fits_u80(U256::ceil_div(numerator1, denominator))
}

/// `getNextSqrtPriceFromAmountYRoundingDown` (addY=true), used by `quoteYToX`.
#[inline]
pub fn get_next_sqrt_price_from_amount_y_rounding_down(
    sqrt_px48: u128,
    liquidity: u128,
    amount_y: U256,
) -> u128 {
    // Solidity branches on amountY <= type(uint80).max for an efficient shift
    // path; for parity we always use the corresponding mulDiv when the input
    // is wider.
    let quotient = if amount_y <= U256::from(U80_MAX) {
        amount_y.shl(48) / U256::from_u128(liquidity)
    } else {
        U256::mul_div(amount_y, Q48, U256::from_u128(liquidity))
    };

    assert_fits_u80(U256::from_u128(sqrt_px48) + quotient)
}

/// |Δx| between two Q32.48 sqrt prices for a given liquidity. Quoting uses
/// `roundUp=false`.
#[inline]
pub fn get_amount_x_delta(
    sqrt_ratio_a: u128,
    sqrt_ratio_b: u128,
    liquidity: u128,
    round_up: bool,
) -> U256 {
    let (sa, sb) = if sqrt_ratio_a > sqrt_ratio_b {
        (sqrt_ratio_b, sqrt_ratio_a)
    } else {
        (sqrt_ratio_a, sqrt_ratio_b)
    };

    assert!(sa != 0, "invalid sqrtRatioAX48");

    let numerator1 = U256::from_u128(liquidity).shl(48);
    let numerator2 = U256::from_u128(sb - sa);
    let sb_u = U256::from_u128(sb);
    let sa_u = U256::from_u128(sa);

    if round_up {
        U256::ceil_div(U256::mul_div_ceil(numerator1, numerator2, sb_u), sa_u)
    } else {
        U256::mul_div(numerator1, numerator2, sb_u) / sa_u
    }
}

/// |Δy| between two Q32.48 sqrt prices for a given liquidity. Quoting uses
/// `roundUp=false`.
#[inline]
pub fn get_amount_y_delta(
    sqrt_ratio_a: u128,
    sqrt_ratio_b: u128,
    liquidity: u128,
    round_up: bool,
) -> U256 {
    let (sa, sb) = if sqrt_ratio_a > sqrt_ratio_b {
        (sqrt_ratio_b, sqrt_ratio_a)
    } else {
        (sqrt_ratio_a, sqrt_ratio_b)
    };

    let diff = U256::from_u128(sb - sa);
    let liq = U256::from_u128(liquidity);

    if round_up {
        U256::mul_div_ceil(liq, diff, Q48)
    } else {
        U256::mul_div(liq, diff, Q48)
    }
}
