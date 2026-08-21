//! Integer-exact off-chain mirror of the Solidity SwapLib quote path.
//!
//! Size slippage follows the LunarBase v2 convention adapted to the v1 pool:
//!
//! * the legacy concentration_k field remains Q20.12 for ABI/storage
//!   compatibility, while effective K = concentration_k / 2^12 is a
//!   protocol-BPS slippage coefficient;
//! * slippageBps = ceil(ceil(ceil(swapCashValue * concentration_k /
//!   poolCashValue) / 2^12) / 10);
//! * slippage is capped at 100,000 protocol BPS (10%);
//! * directional Q24 fees remain a separate v1 component and are applied
//!   after size slippage.

use crate::uint256::{U256Ext, U256};

/// Q48 fixed-point unit (2^48).
pub const Q48: u128 = 1u128 << 48;
/// Q24 fixed-point unit (2^24).
pub const Q24: u128 = 1u128 << 24;
/// Q12 fixed-point unit (2^12).
pub const Q12: u128 = 1u128 << 12;
/// Protocol BPS unit used by LunarBase v2.
pub const BPS: u128 = 1_000_000;
/// V2 slippage scaling divisor.
pub const SLIPPAGE_SCALE: u128 = 10;
/// Maximum size slippage: 10%.
pub const MAX_SLIPPAGE_BPS: u128 = BPS / SLIPPAGE_SCALE;
/// Maximum effective v2-style slippage coefficient accepted by the pool.
pub const MAX_SLIPPAGE_K_BPS: u32 = 1_000_000;
/// Maximum legacy Q20.12 encoding accepted by the pool.
pub const MAX_CONCENTRATION_K: u32 = MAX_SLIPPAGE_K_BPS << 12;

const Q48_U256: U256 = {
    let mut limbs = [0u64; 4];
    limbs[0] = 1u64 << 48;
    U256::from_limbs(limbs)
};
const Q24_U256: U256 = {
    let mut limbs = [0u64; 4];
    limbs[0] = 1u64 << 24;
    U256::from_limbs(limbs)
};
const Q96_U256: U256 = {
    let mut limbs = [0u64; 4];
    limbs[1] = 1u64 << 32;
    U256::from_limbs(limbs)
};

/// Convert a plain effective K into the legacy Q20.12 representation used by
/// PoolParams::concentration_k. Values above the contract cap saturate.
#[inline]
pub fn plain_to_q12_concentration_k(k: u32) -> u32 {
    k.min(MAX_SLIPPAGE_K_BPS) << 12
}

/// Convert a Q20.12 stored concentration_k back to its effective integer K
/// (truncated).
#[inline]
pub fn q12_to_plain_concentration_k(k_q12: u32) -> u32 {
    k_q12 >> 12
}

/// Convert a plain decimal price into a Q64.96 sqrt-price as u128.
///
/// This convenience adapter is lossy beyond f64's 53-bit significand.
/// It panics for negative, NaN, or infinite values and saturates at u128::MAX.
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
    /// Operator-published anchor sqrt-price (Q96, uint160 on-chain).
    ///
    /// The public Rust/N-API adapter supports the contract's commonly used
    /// u128 subset of the full Solidity uint160 range.
    pub sqrt_price_x96: u128,
    /// Fee charged on Y→X swaps in Q24 (uint24 on-chain).
    pub fee_ask_x24: u32,
    /// Fee charged on X→Y swaps in Q24 (uint24 on-chain).
    pub fee_bid_x24: u32,
    /// Reserve of token X (uint112 on-chain).
    pub reserve_x: u128,
    /// Reserve of token Y (uint112 on-chain).
    pub reserve_y: u128,
    /// V2-style slippage coefficient stored under the legacy Q20.12 field.
    pub concentration_k: u32,
}

/// Result of a quote.
pub struct QuoteResult {
    /// Output amount after size slippage and the separate v1 fee.
    pub amount_out: U256,
    /// Informational size-adjusted execution sqrt-price before fee.
    pub sqrt_price_next: u128,
    /// Separate v1 Q24 fee paid in the output token.
    pub fee: U256,
}

/// Mirror of Solidity SwapLib.quoteSlippageBps.
pub fn quote_slippage_bps(
    sqrt_price_x96: u128,
    amount_in: U256,
    reserve_x: u128,
    reserve_y: u128,
    concentration_k: u32,
    x_to_y: bool,
) -> u128 {
    if amount_in.is_zero() || concentration_k == 0 || sqrt_price_x96 == 0 {
        return 0;
    }

    let anchor = U256::from_u128(sqrt_price_x96);
    let x_wealth_in_y = U256::mul_div(
        U256::mul_div(U256::from_u128(reserve_x), anchor, Q96_U256),
        anchor,
        Q96_U256,
    );
    let pool_cash_value = x_wealth_in_y + U256::from_u128(reserve_y);
    if pool_cash_value.is_zero() {
        return 0;
    }

    let swap_cash_value = if x_to_y {
        U256::mul_div(U256::mul_div(amount_in, anchor, Q96_U256), anchor, Q96_U256)
    } else {
        amount_in
    };
    if swap_cash_value.is_zero() {
        return 0;
    }

    // Nested ceilings exactly match v2 for integer K while preserving
    // fractional legacy Q20.12 coefficients.
    let raw_k_q12 = U256::mul_div_ceil(
        swap_cash_value,
        U256::from_u128(concentration_k as u128),
        pool_cash_value,
    );
    let raw_bps = U256::ceil_div(raw_k_q12, U256::from_u128(Q12));
    let slippage_bps = U256::ceil_div(raw_bps, U256::from_u128(SLIPPAGE_SCALE)).as_u128();

    slippage_bps.min(MAX_SLIPPAGE_BPS)
}

/// Mirror of Solidity SwapLib.applySlippage.
pub fn apply_slippage(anchor_output: U256, slippage_bps: u128) -> U256 {
    if anchor_output.is_zero() || slippage_bps == 0 {
        return anchor_output;
    }

    let slippage_amount = U256::mul_div_ceil(
        anchor_output,
        U256::from_u128(slippage_bps),
        U256::from_u128(BPS + slippage_bps),
    );
    anchor_output - slippage_amount
}

fn slippage_sqrt_price(sqrt_price_x96: u128, slippage_bps: u128, x_to_y: bool) -> u128 {
    if sqrt_price_x96 == 0 || slippage_bps == 0 {
        return sqrt_price_x96;
    }

    let ratio_q48 = if x_to_y {
        U256::mul_div(
            U256::from_u128(BPS),
            Q48_U256,
            U256::from_u128(BPS + slippage_bps),
        )
    } else {
        U256::mul_div(
            U256::from_u128(BPS + slippage_bps),
            Q48_U256,
            U256::from_u128(BPS),
        )
    };
    let next = U256::mul_div(
        U256::from_u128(sqrt_price_x96),
        ratio_q48.isqrt(),
        U256::from_u128(Q24),
    );

    if next > U256::from_u128(u128::MAX) {
        u128::MAX
    } else {
        next.as_u128()
    }
}

/// Mirror of Solidity SwapLib.applyFee.
///
/// The Q24 fee is independent from the size-slippage coefficient and is
/// applied after slippage.
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

fn zero_quote(sqrt_price_x96: u128) -> QuoteResult {
    QuoteResult {
        amount_out: U256::ZERO,
        sqrt_price_next: sqrt_price_x96,
        fee: U256::ZERO,
    }
}

/// Quote a token-X-in / token-Y-out exact-input swap with the base bid fee.
pub fn quote_x_to_y(params: &PoolParams, dx: U256) -> QuoteResult {
    quote_x_to_y_with_multiplier(params, dx, U256::from(1u64))
}

/// Quote a token-Y-in / token-X-out exact-input swap with the base ask fee.
pub fn quote_y_to_x(params: &PoolParams, dy: U256) -> QuoteResult {
    quote_y_to_x_with_multiplier(params, dy, U256::from(1u64))
}

/// Quote X→Y with an explicit caller fee multiplier.
pub fn quote_x_to_y_with_multiplier(
    params: &PoolParams,
    dx: U256,
    fee_multiplier: U256,
) -> QuoteResult {
    let anchor = U256::from_u128(params.sqrt_price_x96);
    let anchor_output = U256::mul_div(U256::mul_div(dx, anchor, Q96_U256), anchor, Q96_U256);
    let slippage_bps = quote_slippage_bps(
        params.sqrt_price_x96,
        dx,
        params.reserve_x,
        params.reserve_y,
        params.concentration_k,
        true,
    );
    let gross_output = apply_slippage(anchor_output, slippage_bps);
    if gross_output.is_zero() || gross_output > U256::from_u128(params.reserve_y) {
        return zero_quote(params.sqrt_price_x96);
    }

    let (amount_out, fee) = apply_fee(gross_output, params.fee_bid_x24, fee_multiplier);
    QuoteResult {
        amount_out,
        sqrt_price_next: slippage_sqrt_price(params.sqrt_price_x96, slippage_bps, true),
        fee,
    }
}

/// Quote Y→X with an explicit caller fee multiplier.
pub fn quote_y_to_x_with_multiplier(
    params: &PoolParams,
    dy: U256,
    fee_multiplier: U256,
) -> QuoteResult {
    if params.sqrt_price_x96 == 0 {
        return zero_quote(params.sqrt_price_x96);
    }

    let anchor = U256::from_u128(params.sqrt_price_x96);
    let anchor_output = U256::mul_div(U256::mul_div(dy, Q96_U256, anchor), Q96_U256, anchor);
    let slippage_bps = quote_slippage_bps(
        params.sqrt_price_x96,
        dy,
        params.reserve_x,
        params.reserve_y,
        params.concentration_k,
        false,
    );
    let gross_output = apply_slippage(anchor_output, slippage_bps);
    if gross_output.is_zero() || gross_output > U256::from_u128(params.reserve_x) {
        return zero_quote(params.sqrt_price_x96);
    }

    let (amount_out, fee) = apply_fee(gross_output, params.fee_ask_x24, fee_multiplier);
    QuoteResult {
        amount_out,
        sqrt_price_next: slippage_sqrt_price(params.sqrt_price_x96, slippage_bps, false),
        fee,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(concentration_k: u32) -> PoolParams {
        PoolParams {
            sqrt_price_x96: 1u128 << 96,
            fee_ask_x24: 0,
            fee_bid_x24: ((5u32) * (1u32 << 24)) / 100,
            reserve_x: 1_000_000,
            reserve_y: 1_000_000,
            concentration_k,
        }
    }

    #[test]
    fn helper_saturates_at_contract_k_cap() {
        assert_eq!(
            plain_to_q12_concentration_k(MAX_SLIPPAGE_K_BPS + 1),
            MAX_CONCENTRATION_K
        );
    }

    #[test]
    fn slippage_is_zero_when_inputs_are_zero() {
        assert_eq!(
            quote_slippage_bps(1u128 << 96, U256::ZERO, 10_000, 10_000, 5_000 << 12, true,),
            0
        );
        assert_eq!(
            quote_slippage_bps(1u128 << 96, U256::from(1_000u64), 10_000, 10_000, 0, true,),
            0
        );
        assert_eq!(
            quote_slippage_bps(0, U256::from(1_000u64), 10_000, 10_000, 5_000 << 12, true,),
            0
        );
    }

    #[test]
    fn slippage_is_linear_and_uses_v2_rounding() {
        let k = 50_000u32 << 12;
        let first = quote_slippage_bps(
            1u128 << 96,
            U256::from(100_000u64),
            1_000_000,
            1_000_000,
            k,
            true,
        );
        let second = quote_slippage_bps(
            1u128 << 96,
            U256::from(200_000u64),
            1_000_000,
            1_000_000,
            k,
            true,
        );
        assert_eq!(first, 250);
        assert_eq!(second, 500);
    }

    #[test]
    fn slippage_preserves_fractional_legacy_q20_12_k() {
        assert_eq!(
            quote_slippage_bps(1u128 << 96, U256::from(2u64), 1, 1, 100, true),
            1
        );
    }

    #[test]
    fn slippage_caps_at_ten_percent() {
        assert_eq!(
            quote_slippage_bps(
                1u128 << 96,
                U256::from(2u64),
                1,
                1,
                MAX_CONCENTRATION_K,
                true,
            ),
            MAX_SLIPPAGE_BPS
        );
    }

    #[test]
    fn quote_x_to_y_uses_anchor_when_k_zero() {
        let result = quote_x_to_y(&params(0), U256::from(1_000u64));
        assert_eq!(result.fee, U256::from(49u64));
        assert_eq!(result.amount_out, U256::from(951u64));
        assert_eq!(result.sqrt_price_next, 1u128 << 96);
    }

    #[test]
    fn quote_applies_slippage_before_separate_fee() {
        let p = params(50_000u32 << 12);
        let amount_in = U256::from(100_000u64);
        let slippage_bps = quote_slippage_bps(
            p.sqrt_price_x96,
            amount_in,
            p.reserve_x,
            p.reserve_y,
            p.concentration_k,
            true,
        );
        let gross = apply_slippage(amount_in, slippage_bps);
        let result = quote_x_to_y(&p, amount_in);
        let expected_fee = U256::mul_div(gross, U256::from_u128(p.fee_bid_x24 as u128), Q24_U256);

        assert_eq!(result.fee, expected_fee);
        assert_eq!(result.amount_out, gross - expected_fee);
        assert!(result.sqrt_price_next < p.sqrt_price_x96);
    }

    #[test]
    fn fee_multiplier_doubles_fee() {
        let p = params(0);
        let base = quote_x_to_y(&p, U256::from(1_000u64));
        let scaled = quote_x_to_y_with_multiplier(&p, U256::from(1_000u64), U256::from(2u64));
        assert_eq!(scaled.fee, base.fee * U256::from(2u64));
        assert_eq!(scaled.amount_out, U256::from(1_000u64) - scaled.fee);
    }

    #[test]
    fn fee_multiplier_consumes_all_output_when_too_large() {
        let result =
            quote_x_to_y_with_multiplier(&params(0), U256::from(1_000u64), U256::from(100u64));
        assert_eq!(result.amount_out, U256::ZERO);
        assert_eq!(result.fee, U256::from(1_000u64));
    }

    #[test]
    fn quote_returns_zero_when_no_liquidity() {
        let mut p = params(5_000u32 << 12);
        p.reserve_x = 0;
        p.reserve_y = 0;
        let result = quote_x_to_y(&p, U256::from(1_000u64));
        assert_eq!(result.amount_out, U256::ZERO);
    }
}
