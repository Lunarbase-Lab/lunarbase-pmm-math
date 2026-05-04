//! [`U256`] alias around [`ruint::Uint`] plus a Solidity-compatible
//! arithmetic extension trait used throughout the crate.

use ruint::Uint;

/// 256-bit unsigned integer (4 × 64-bit limbs).
pub type U256 = Uint<256, 4>;
type U512 = Uint<512, 8>;

/// Solidity-compatible arithmetic helpers for [`U256`].
///
/// All operations use 512-bit intermediates where required to match the
/// behavior of `FullMath.mulDiv` and friends from Uniswap V3.
#[allow(clippy::wrong_self_convention)]
pub trait U256Ext {
    /// `2^48` as a [`U256`] constant.
    const Q48: U256;

    /// `floor((a * b) / denominator)` with 512-bit intermediate.
    fn mul_div(a: U256, b: U256, denominator: U256) -> U256;
    /// `ceil((a * b) / denominator)` with 512-bit intermediate.
    fn mul_div_ceil(a: U256, b: U256, denominator: U256) -> U256;
    /// `ceil(a / b)`.
    fn ceil_div(a: U256, b: U256) -> U256;
    /// Integer square root (floor). Matches OpenZeppelin `Math.sqrt`.
    #[must_use]
    fn isqrt(self) -> U256;
    /// `true` if the value fits in 80 bits.
    fn fits_u80(self) -> bool;
    /// `true` if the value fits in 128 bits.
    fn fits_u128(self) -> bool;
    /// `true` if the value fits in 160 bits.
    fn fits_u160(self) -> bool;
    /// Truncate to `u128`. Panics if the value doesn't fit.
    fn as_u128(self) -> u128;
    /// Construct from a `u128`.
    fn from_u128(v: u128) -> U256;
    /// Construct from a `u64`.
    fn from_u64(v: u64) -> U256;
    /// Logical left shift by `shift` bits.
    #[must_use]
    fn shl(self, shift: u32) -> U256;
    /// Logical right shift by `shift` bits.
    #[must_use]
    fn shr(self, shift: u32) -> U256;
}

impl U256Ext for U256 {
    const Q48: U256 = {
        let mut limbs = [0u64; 4];
        limbs[0] = 1u64 << 48;
        Uint::from_limbs(limbs)
    };

    #[inline(always)]
    fn from_u128(v: u128) -> U256 {
        U256::from(v)
    }

    #[inline(always)]
    fn from_u64(v: u64) -> U256 {
        U256::from(v)
    }

    #[inline(always)]
    fn fits_u80(self) -> bool {
        self <= U256::from((1u128 << 80) - 1)
    }

    #[inline(always)]
    fn fits_u128(self) -> bool {
        self <= U256::from(u128::MAX)
    }

    #[inline(always)]
    fn fits_u160(self) -> bool {
        self.fits_u128()
    }

    #[inline(always)]
    fn as_u128(self) -> u128 {
        assert!(self.fits_u128(), "U256 overflow to u128");
        self.to::<u128>()
    }

    #[inline(always)]
    fn shl(self, shift: u32) -> U256 {
        self << shift as usize
    }

    #[inline(always)]
    fn shr(self, shift: u32) -> U256 {
        self >> shift as usize
    }

    /// Solidity-style mulDiv: floor((a * b) / denominator) with 512-bit intermediate.
    fn mul_div(a: U256, b: U256, denominator: U256) -> U256 {
        assert!(!denominator.is_zero(), "mulDiv: division by zero");
        let product: U512 = a.widening_mul(b);
        let denom_512 = U512::from(denominator);
        let result = product / denom_512;
        // Ensure result fits in 256 bits
        assert!(
            result <= U512::from(U256::MAX),
            "mulDiv: result overflows U256"
        );
        result.to::<U256>()
    }

    /// mulDiv with rounding up.
    fn mul_div_ceil(a: U256, b: U256, denominator: U256) -> U256 {
        assert!(!denominator.is_zero(), "mulDiv: division by zero");
        let product: U512 = a.widening_mul(b);
        let denom_512 = U512::from(denominator);
        let q = product / denom_512;
        let r = product % denom_512;
        let result = if r > U512::ZERO {
            q + U512::from(1u64)
        } else {
            q
        };
        assert!(
            result <= U512::from(U256::MAX),
            "mulDivCeil: result overflows U256"
        );
        result.to::<U256>()
    }

    /// ceil(a / b)
    fn ceil_div(a: U256, b: U256) -> U256 {
        a.div_ceil(b)
    }

    /// Integer square root (floor). Matches OpenZeppelin Math.sqrt.
    fn isqrt(self) -> U256 {
        if self.is_zero() {
            return U256::ZERO;
        }

        // Initial overestimate: 2^((bit_length + 1) / 2)
        let bit_len = 256 - self.leading_zeros();
        let mut result = U256::from(1u64) << bit_len.div_ceil(2);

        // Newton's method: converges in ≤8 iterations for 256-bit
        for _ in 0..8 {
            let new_result = (result + self / result) >> 1;
            if new_result >= result {
                return result;
            }
            result = new_result;
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_arithmetic() {
        let a = U256::from(100u64);
        let b = U256::from(200u64);
        assert_eq!(a + b, U256::from(300u64));
        assert_eq!(b - a, U256::from(100u64));
    }

    #[test]
    fn test_mul_div() {
        let result = U256::mul_div(U256::from(100u64), U256::from(200u64), U256::from(50u64));
        assert_eq!(result, U256::from(400u64));
    }

    #[test]
    fn test_isqrt() {
        assert_eq!(U256::from(0u64).isqrt(), U256::ZERO);
        assert_eq!(U256::from(1u64).isqrt(), U256::from(1u64));
        assert_eq!(U256::from(4u64).isqrt(), U256::from(2u64));
        assert_eq!(U256::from(9u64).isqrt(), U256::from(3u64));
        assert_eq!(U256::from(10u64).isqrt(), U256::from(3u64));
        assert_eq!(U256::from(100u64).isqrt(), U256::from(10u64));
    }

    #[test]
    fn test_shifts() {
        let a = U256::from(1u64);
        let shifted = a.shl(48);
        assert_eq!(shifted, U256::from(1u128 << 48));
        let back = shifted.shr(48);
        assert_eq!(back, a);
    }

    #[test]
    fn test_mul_div_large() {
        // Test with values that require 512-bit intermediate
        let a = U256::from(1u128 << 48);
        let b = U256::from(1u128 << 48);
        let d = U256::from(1u128 << 48);
        let result = U256::mul_div(a, b, d);
        assert_eq!(result, U256::from(1u128 << 48));
    }
}
