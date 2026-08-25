package lunarbasepmm

import (
	"fmt"
	"math"

	"github.com/holiman/uint256"
)

var (
	one                 = uint256.NewInt(1)
	q24                 = new(uint256.Int).Lsh(one, 24)
	q96                 = new(uint256.Int).Lsh(one, 96)
	maxSqrtPriceX96U160 = new(uint256.Int).Sub(new(uint256.Int).Lsh(one, 160), one)
)

// mulDivDownChecked computes floor(x*y/denominator) with a full 512-bit
// intermediate. It reports the same conditions for which Solidity Math.mulDiv
// reverts instead of silently returning a truncated quotient.
func mulDivDownChecked(dst, x, y, denominator *uint256.Int) error {
	if dst == nil || x == nil || y == nil || denominator == nil {
		return fmt.Errorf("%w: nil mulDiv operand", ErrInvalidArgument)
	}
	if denominator.IsZero() {
		dst.Clear()
		return ErrDivisionByZero
	}
	if _, overflow := dst.MulDivOverflow(x, y, denominator); overflow {
		dst.Clear()
		return ErrMathOverflow
	}
	return nil
}

// mulDivUpChecked computes ceil(x*y/denominator) and checks both quotient and
// rounding-increment overflow.
func mulDivUpChecked(dst, x, y, denominator *uint256.Int) error {
	if dst == nil || x == nil || y == nil || denominator == nil {
		return fmt.Errorf("%w: nil mulDiv operand", ErrInvalidArgument)
	}
	if denominator.IsZero() {
		dst.Clear()
		return ErrDivisionByZero
	}
	var remainder uint256.Int
	remainder.MulMod(x, y, denominator)
	if _, overflow := dst.MulDivOverflow(x, y, denominator); overflow {
		dst.Clear()
		return ErrMathOverflow
	}
	if !remainder.IsZero() {
		if _, overflow := dst.AddOverflow(dst, one); overflow {
			dst.Clear()
			return ErrMathOverflow
		}
	}
	return nil
}

// PriceToSqrtPriceX96 converts a plain decimal price (e.g. 2500.0) into a
// Q64.96 sqrt-price. Lossy beyond float64's 53-bit significand. Panics on
// NaN/Inf/negative and saturates at uint160.max, matching the runtime domain.
func PriceToSqrtPriceX96(price float64) *uint256.Int {
	if math.IsNaN(price) || math.IsInf(price, 0) || price < 0 {
		panic("price must be finite and non-negative")
	}
	scaled := math.Sqrt(price) * math.Pow(2, 96)
	if scaled >= math.Ldexp(1, 160) {
		return new(uint256.Int).Set(maxSqrtPriceX96U160)
	}
	return f64FloorToU256(scaled)
}

// SqrtPriceX96ToPrice converts a Q64.96 sqrt-price back to a plain decimal
// price ((p/2^96)^2). Lossy beyond float64's 53-bit significand. Pass nil
// through as 0.
func SqrtPriceX96ToPrice(pX96 *uint256.Int) float64 {
	if pX96 == nil {
		return 0
	}
	sqrtP := u256ToF64Lossy(pX96) / math.Pow(2, 96)
	return sqrtP * sqrtP
}

// f64FloorToU256 decodes a finite, non-negative float64 to floor(x) as a
// uint256.Int. Returns zero for x < 1 (and for NaN / -Inf, which callers
// should reject). Saturates at 2^256-1 on overflow.
func f64FloorToU256(x float64) *uint256.Int {
	if math.IsNaN(x) || math.IsInf(x, 0) || x < 1 {
		return new(uint256.Int)
	}
	bits := math.Float64bits(x)
	exp := int((bits>>52)&0x7ff) - 1023
	mantissa := (bits & ((1 << 52) - 1)) | (1 << 52)
	out := new(uint256.Int).SetUint64(mantissa)
	if exp >= 52 {
		shift := uint(exp - 52)
		if shift >= 256-53 {
			max := new(uint256.Int)
			max.Not(max)
			return max
		}
		return out.Lsh(out, shift)
	}
	return out.Rsh(out, uint(52-exp))
}

// u256ToF64Lossy converts a uint256.Int to float64 by keeping the top ~53
// bits of significand. Lossy for values above 2^53.
func u256ToF64Lossy(v *uint256.Int) float64 {
	if v.IsZero() {
		return 0
	}
	bitLen := v.BitLen()
	if bitLen <= 64 {
		return float64(v.Uint64())
	}
	shift := uint(bitLen - 53)
	truncated := new(uint256.Int).Rsh(v, shift)
	return float64(truncated.Uint64()) * math.Ldexp(1, int(shift))
}
