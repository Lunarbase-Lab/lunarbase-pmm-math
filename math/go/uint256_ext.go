package lunarbasepmm

import "github.com/holiman/uint256"

const fixedPoint48Resolution = 48

var (
	one     = uint256.NewInt(1)
	q12     = new(uint256.Int).Lsh(one, 12)
	q24     = new(uint256.Int).Lsh(one, 24)
	q48     = new(uint256.Int).Lsh(one, 48)
	q96     = new(uint256.Int).Lsh(one, 96)
	u2Pow80 = new(uint256.Int).Lsh(one, 80)
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
