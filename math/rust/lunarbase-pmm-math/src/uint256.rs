//! [`U256`] alias around [`ruint::Uint`] plus a Solidity-compatible
//! arithmetic extension trait used throughout the crate.

use ruint::Uint;

/// 256-bit unsigned integer (4 × 64-bit limbs).
pub type U256 = Uint<256, 4>;
type U512 = Uint<512, 8>;

/// Solidity-compatible arithmetic helpers for [`U256`].
///
/// All operations use 512-bit intermediates where required to match the
/// behavior of OpenZeppelin `Math.mulDiv` used by the current mechanism.
#[allow(clippy::wrong_self_convention)]
pub trait U256Ext {
    /// `floor((a * b) / denominator)` with 512-bit intermediate.
    fn mul_div(a: U256, b: U256, denominator: U256) -> U256;
    /// Checked `floor((a * b) / denominator)` with 512-bit intermediate.
    ///
    /// Returns `None` when the denominator is zero or the quotient does not
    /// fit in 256 bits, matching the two revert conditions of Solidity
    /// `Math.mulDiv`.
    fn checked_mul_div(a: U256, b: U256, denominator: U256) -> Option<U256>;
    /// `ceil((a * b) / denominator)` with 512-bit intermediate.
    fn mul_div_ceil(a: U256, b: U256, denominator: U256) -> U256;
    /// Checked `ceil((a * b) / denominator)` with 512-bit intermediate.
    fn checked_mul_div_ceil(a: U256, b: U256, denominator: U256) -> Option<U256>;
    /// `true` if the value fits in 128 bits.
    fn fits_u128(self) -> bool;
    /// `true` if the value fits in 112 bits.
    fn fits_u112(self) -> bool;
    /// `true` if the value fits in 160 bits.
    fn fits_u160(self) -> bool;
    /// Convert to `u128`. Panics if the value doesn't fit.
    fn as_u128(self) -> u128;
}

impl U256Ext for U256 {
    #[inline(always)]
    fn fits_u128(self) -> bool {
        self <= U256::from(u128::MAX)
    }

    #[inline(always)]
    fn fits_u112(self) -> bool {
        (self >> 112) == U256::ZERO
    }

    #[inline(always)]
    fn fits_u160(self) -> bool {
        (self >> 160) == U256::ZERO
    }

    #[inline(always)]
    fn as_u128(self) -> u128 {
        assert!(self.fits_u128(), "U256 overflow to u128");
        self.to::<u128>()
    }

    /// Solidity-style mulDiv: floor((a * b) / denominator) with 512-bit intermediate.
    #[inline]
    fn mul_div(a: U256, b: U256, denominator: U256) -> U256 {
        Self::checked_mul_div(a, b, denominator)
            .expect("mulDiv: division by zero or result overflows U256")
    }

    #[inline]
    fn checked_mul_div(a: U256, b: U256, denominator: U256) -> Option<U256> {
        if denominator.is_zero() {
            return None;
        }
        let product: U512 = a.widening_mul(b);
        let denom_512 = U512::from(denominator);
        let result = product / denom_512;
        if result > U512::from(U256::MAX) {
            return None;
        }
        Some(result.to::<U256>())
    }

    /// mulDiv with rounding up.
    #[inline]
    fn mul_div_ceil(a: U256, b: U256, denominator: U256) -> U256 {
        Self::checked_mul_div_ceil(a, b, denominator)
            .expect("mulDivCeil: division by zero or result overflows U256")
    }

    #[inline]
    fn checked_mul_div_ceil(a: U256, b: U256, denominator: U256) -> Option<U256> {
        if denominator.is_zero() {
            return None;
        }
        let product: U512 = a.widening_mul(b);
        let denom_512 = U512::from(denominator);
        let q = product / denom_512;
        let r = product % denom_512;
        let result = if r > U512::ZERO {
            if q == U512::MAX {
                return None;
            }
            q + U512::from(1u64)
        } else {
            q
        };
        if result > U512::from(U256::MAX) {
            return None;
        }
        Some(result.to::<U256>())
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
    fn test_mul_div_large() {
        // Test with values that require 512-bit intermediate
        let a = U256::from(1u128 << 48);
        let b = U256::from(1u128 << 48);
        let d = U256::from(1u128 << 48);
        let result = U256::mul_div(a, b, d);
        assert_eq!(result, U256::from(1u128 << 48));
    }

    #[test]
    fn checked_mul_div_reports_revert_conditions() {
        assert_eq!(
            U256::checked_mul_div(U256::from(1u64), U256::from(1u64), U256::ZERO),
            None
        );
        assert_eq!(
            U256::checked_mul_div(U256::MAX, U256::MAX, U256::from(1u64)),
            None
        );
        assert_eq!(
            U256::checked_mul_div_ceil(U256::from(10u64), U256::from(10u64), U256::from(6u64)),
            Some(U256::from(17u64))
        );
    }

    #[test]
    fn solidity_width_checks_cover_u112_and_full_u160() {
        assert!((U256::from(1u64) << 111usize).fits_u112());
        assert!(!(U256::from(1u64) << 112usize).fits_u112());
        assert!((U256::from(1u64) << 159usize).fits_u160());
        assert!(!(U256::from(1u64) << 160usize).fits_u160());
    }
}
