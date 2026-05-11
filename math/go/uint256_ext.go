package lunarbasepmm

import (
	"math"

	"github.com/holiman/uint256"
)

const fixedPoint96Resolution = 96

var (
	one      = uint256.NewInt(1)
	q12      = new(uint256.Int).Lsh(one, 12)
	q24      = new(uint256.Int).Lsh(one, 24)
	q48      = new(uint256.Int).Lsh(one, 48)
	q96      = new(uint256.Int).Lsh(one, 96)
	u2Pow160 = new(uint256.Int).Lsh(one, 160)
)

// mulDivDown computes floor(x*y/denominator) into dst with a 512-bit
// intermediate. Mirrors Solidity `FullMath.mulDiv` (round-down). Returns dst
// for chaining. Aliasing dst with x or y is safe — `holiman/uint256`'s
// MulDivOverflow handles it internally.
func mulDivDown(dst, x, y, denominator *uint256.Int) *uint256.Int {
	dst.MulDivOverflow(x, y, denominator)
	return dst
}

// mulDivUp computes ceil(x*y/denominator) into dst with a 512-bit intermediate.
// Mirrors Solidity `FullMath.mulDivRoundingUp`.
func mulDivUp(dst, x, y, denominator *uint256.Int) *uint256.Int {
	var rem uint256.Int
	rem.MulMod(x, y, denominator)
	dst.MulDivOverflow(x, y, denominator)
	if !rem.IsZero() {
		dst.AddUint64(dst, 1)
	}
	return dst
}

// ceilDiv computes ceil(a/b) into dst.
func ceilDiv(dst, a, b *uint256.Int) *uint256.Int {
	var rem uint256.Int
	dst.DivMod(a, b, &rem)
	if !rem.IsZero() {
		dst.AddUint64(dst, 1)
	}
	return dst
}

// isqrt computes floor(sqrt(x)) into dst.
func isqrt(dst, x *uint256.Int) *uint256.Int {
	return dst.Sqrt(x)
}

// SqrtPriceX48ToX96 lifts a Q32.48 sqrt-price (legacy pX48, uint80) into a
// Q64.96 sqrt-price (pX96, uint160) by shifting left 48 bits. The result
// represents the same numerical price (same value of (p/Q)^2). Pass nil
// through unchanged for ergonomics.
func SqrtPriceX48ToX96(pX48 *uint256.Int) *uint256.Int {
	if pX48 == nil {
		return nil
	}
	out := new(uint256.Int).Set(pX48)
	return out.Lsh(out, 48)
}

// SqrtPriceX96ToX48 lowers a Q64.96 sqrt-price (pX96, uint160) into a Q32.48
// sqrt-price (pX48, uint80) by right-shifting 48 bits, truncating the bottom
// 48 bits of precision. Used for backward-compat with legacy serialised
// state. Pass nil through unchanged.
func SqrtPriceX96ToX48(pX96 *uint256.Int) *uint256.Int {
	if pX96 == nil {
		return nil
	}
	out := new(uint256.Int).Set(pX96)
	return out.Rsh(out, 48)
}

// PlainToQ12ConcentrationK lifts a plain effective K (no fractional part)
// into the Q20.12 representation expected by `PoolParams.ConcentrationK`.
// `PlainToQ12ConcentrationK(100) == 409_600`. Saturates at math.MaxUint32
// if the shift would overflow.
func PlainToQ12ConcentrationK(k uint32) uint32 {
	const limit = uint32(1) << 20 // (math.MaxUint32 >> 12) + 1
	if k >= limit {
		return ^uint32(0)
	}
	return k << 12
}

// Q12ToPlainConcentrationK reverses [PlainToQ12ConcentrationK] (truncates the
// fractional part). `Q12ToPlainConcentrationK(409_600) == 100`.
func Q12ToPlainConcentrationK(kQ12 uint32) uint32 {
	return kQ12 >> 12
}

// PriceToSqrtPriceX96 converts a plain decimal price (e.g. 2500.0) into a
// Q64.96 sqrt-price (uint160). Lossy beyond float64's 53-bit significand.
// Panics if price is negative, NaN, or +/-Inf. Saturates at 2^256-1 on
// overflow.
func PriceToSqrtPriceX96(price float64) *uint256.Int {
	if math.IsNaN(price) || math.IsInf(price, 0) || price < 0 {
		panic("price must be finite and non-negative")
	}
	scaled := math.Sqrt(price) * math.Pow(2, 96)
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

// PriceToSqrtPriceX48 converts a plain decimal price into a Q32.48 sqrt-price
// (uint80) as *uint256.Int. Lossy beyond float64's 53-bit significand. Panics
// on NaN/Inf/negative; saturates at 2^80-1 on overflow.
func PriceToSqrtPriceX48(price float64) *uint256.Int {
	if math.IsNaN(price) || math.IsInf(price, 0) || price < 0 {
		panic("price must be finite and non-negative")
	}
	scaled := math.Sqrt(price) * math.Pow(2, 48)
	u80Max := new(uint256.Int).Sub(new(uint256.Int).Lsh(one, 80), one)
	if !(scaled >= 0) || math.IsInf(scaled, 0) {
		return new(uint256.Int)
	}
	u80MaxF := math.Ldexp(1, 80)
	if scaled >= u80MaxF {
		return u80Max
	}
	return new(uint256.Int).SetUint64(uint64(scaled))
}

// SqrtPriceX48ToPrice converts a Q32.48 sqrt-price (uint80) back to a plain
// decimal price. Pass nil through as 0.
func SqrtPriceX48ToPrice(pX48 *uint256.Int) float64 {
	if pX48 == nil {
		return 0
	}
	sqrtP := u256ToF64Lossy(pX48) / math.Pow(2, 48)
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
