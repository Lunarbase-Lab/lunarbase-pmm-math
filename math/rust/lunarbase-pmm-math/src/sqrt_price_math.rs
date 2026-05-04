//! Uniswap V3-style sqrt-price arithmetic ported to Q48 fixed-point.
//!
//! All functions here mirror the on-chain `SqrtPriceMath` library and match
//! the rounding modes used by `CurvePMM.quoteXToY` / `quoteYToX`.

use crate::uint256::{U256Ext, U256};

const Q48: U256 = U256::Q48;

/// `getNextSqrtPriceFromAmountXRoundingUp` (addX=true), used by `quoteXToY`.
pub fn get_next_sqrt_price_from_amount_x_rounding_up(
    sqrt_px48: u128,
    liquidity: u128,
    amount_x: U256,
) -> u128 {
    if amount_x.is_zero() {
        return sqrt_px48;
    }

    let numerator1 = U256::from_u128(liquidity).shl(48);
    let sqrt_p = U256::from_u128(sqrt_px48);

    // product = amountX * sqrtPX48 (unchecked in Solidity, wrapping here)
    let product = amount_x.wrapping_mul(sqrt_p);

    // Check if product / amountX == sqrtPX48 (no overflow)
    if !amount_x.is_zero() && product / amount_x == sqrt_p {
        let denominator = numerator1.wrapping_add(product);
        if denominator >= numerator1 {
            // mulDiv(numerator1, sqrtPX48, denominator, Ceil)
            let result = U256::mul_div_ceil(numerator1, sqrt_p, denominator);
            assert!(result.fits_u80(), "sqrt price overflow u80");
            return result.as_u128();
        }
    }

    // Fallback: ceilDiv(numerator1, numerator1/sqrtPX48 + amountX)
    let div_result = numerator1 / sqrt_p;
    let denominator = div_result.wrapping_add(amount_x);
    let result = U256::ceil_div(numerator1, denominator);
    assert!(result.fits_u80(), "sqrt price overflow u80");
    result.as_u128()
}

/// `getNextSqrtPriceFromAmountYRoundingDown` (addY=true), used by `quoteYToX`.
pub fn get_next_sqrt_price_from_amount_y_rounding_down(
    sqrt_px48: u128,
    liquidity: u128,
    amount_y: U256,
) -> u128 {
    let quotient = if amount_y.fits_u80() {
        // (amountY << 48) / liquidity
        amount_y.shl(48) / U256::from_u128(liquidity)
    } else {
        U256::mul_div(amount_y, Q48, U256::from_u128(liquidity))
    };

    let result = U256::from_u128(sqrt_px48) + quotient;
    assert!(result.fits_u80(), "sqrt price overflow u80");
    result.as_u128()
}

/// |Δx| between two sqrt prices for a given liquidity. Quoting uses `roundUp=false`.
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

    if round_up {
        U256::ceil_div(
            U256::mul_div_ceil(numerator1, numerator2, U256::from_u128(sb)),
            U256::from_u128(sa),
        )
    } else {
        U256::mul_div(numerator1, numerator2, U256::from_u128(sb)) / U256::from_u128(sa)
    }
}

/// |Δy| between two sqrt prices for a given liquidity. Quoting uses `roundUp=false`.
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

    if round_up {
        U256::mul_div_ceil(U256::from_u128(liquidity), diff, Q48)
    } else {
        U256::mul_div(U256::from_u128(liquidity), diff, Q48)
    }
}
