//! Uniswap V3-style sqrt-price arithmetic in Q64.96 fixed-point.
//!
//! All functions here mirror the on-chain `SqrtPriceMath` library on the
//! `fix/incident` branch (single-price Q64.96 design) and match the rounding
//! modes used by `SwapLib._quoteXToY` / `_quoteYToX`.

use crate::uint256::{U256Ext, U256};

const Q96: U256 = U256::Q96;

/// `getNextSqrtPriceFromAmountXRoundingUp` (addX=true), used by `quoteXToY`.
/// Sqrt-prices are uint160 (Q64.96); stored as `U256` to safely hold
/// intermediate products. Result asserted to fit uint160 (≤ 2^160 − 1).
#[inline]
pub fn get_next_sqrt_price_from_amount_x_rounding_up(
    sqrt_px96: U256,
    liquidity: u128,
    amount_x: U256,
) -> U256 {
    if amount_x.is_zero() {
        return sqrt_px96;
    }

    // numerator1 = liquidity << 96
    let numerator1 = U256::from_u128(liquidity).shl(96);

    // product = amountX * sqrtPX96 (unchecked wrap matching Solidity)
    let product = amount_x.wrapping_mul(sqrt_px96);

    // Overflow-check: product / amountX == sqrtPX96
    if !amount_x.is_zero() && product / amount_x == sqrt_px96 {
        let denominator = numerator1.wrapping_add(product);
        if denominator >= numerator1 {
            let result = U256::mul_div_ceil(numerator1, sqrt_px96, denominator);
            assert!(result.fits_u160(), "sqrt price overflow u160");
            return result;
        }
    }

    // Fallback: ceilDiv(numerator1, numerator1/sqrtPX96 + amountX)
    let div_result = numerator1 / sqrt_px96;
    let denominator = div_result.wrapping_add(amount_x);
    let result = U256::ceil_div(numerator1, denominator);
    assert!(result.fits_u160(), "sqrt price overflow u160");
    result
}

/// `getNextSqrtPriceFromAmountYRoundingDown` (addY=true), used by `quoteYToX`.
#[inline]
pub fn get_next_sqrt_price_from_amount_y_rounding_down(
    sqrt_px96: U256,
    liquidity: u128,
    amount_y: U256,
) -> U256 {
    // Solidity branches on amountY <= type(uint160).max for an efficient shift
    // path; for parity we always use mulDiv (it produces the same result with
    // 256-bit U256 here).
    let quotient = if amount_y.fits_u160() {
        amount_y.shl(96) / U256::from_u128(liquidity)
    } else {
        U256::mul_div(amount_y, Q96, U256::from_u128(liquidity))
    };

    let result = sqrt_px96 + quotient;
    assert!(result.fits_u160(), "sqrt price overflow u160");
    result
}

/// |Δx| between two sqrt prices for a given liquidity. Quoting uses `roundUp=false`.
#[inline]
pub fn get_amount_x_delta(
    sqrt_ratio_a: U256,
    sqrt_ratio_b: U256,
    liquidity: u128,
    round_up: bool,
) -> U256 {
    let (sa, sb) = if sqrt_ratio_a > sqrt_ratio_b {
        (sqrt_ratio_b, sqrt_ratio_a)
    } else {
        (sqrt_ratio_a, sqrt_ratio_b)
    };

    assert!(!sa.is_zero(), "invalid sqrtRatioAX96");

    let numerator1 = U256::from_u128(liquidity).shl(96);
    let numerator2 = sb - sa;

    if round_up {
        U256::ceil_div(U256::mul_div_ceil(numerator1, numerator2, sb), sa)
    } else {
        U256::mul_div(numerator1, numerator2, sb) / sa
    }
}

/// |Δy| between two sqrt prices for a given liquidity. Quoting uses `roundUp=false`.
#[inline]
pub fn get_amount_y_delta(
    sqrt_ratio_a: U256,
    sqrt_ratio_b: U256,
    liquidity: u128,
    round_up: bool,
) -> U256 {
    let (sa, sb) = if sqrt_ratio_a > sqrt_ratio_b {
        (sqrt_ratio_b, sqrt_ratio_a)
    } else {
        (sqrt_ratio_a, sqrt_ratio_b)
    };

    let diff = sb - sa;

    if round_up {
        U256::mul_div_ceil(U256::from_u128(liquidity), diff, Q96)
    } else {
        U256::mul_div(U256::from_u128(liquidity), diff, Q96)
    }
}
