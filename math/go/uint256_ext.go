package lunarbasepmm

import "github.com/holiman/uint256"

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
