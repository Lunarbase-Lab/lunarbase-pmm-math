package lunarbasepmm

import "github.com/holiman/uint256"

const fixedPoint48Resolution = 48

var (
	one     = uint256.NewInt(1)
	q24     = new(uint256.Int).Lsh(one, 24)
	q48     = new(uint256.Int).Lsh(one, 48)
	u2Pow80 = new(uint256.Int).Lsh(one, 80)
)

// mulDivDown computes floor(x*y/denominator) with a 512-bit intermediate.
// Mirrors Solidity FullMath.mulDiv (round-down).
func mulDivDown(x, y, denominator *uint256.Int) *uint256.Int {
	res := new(uint256.Int)
	res.MulDivOverflow(x, y, denominator)
	return res
}

// mulDivUp computes ceil(x*y/denominator) with a 512-bit intermediate.
// Mirrors Solidity FullMath.mulDivRoundingUp.
func mulDivUp(x, y, denominator *uint256.Int) *uint256.Int {
	res := new(uint256.Int)
	res.MulDivOverflow(x, y, denominator)
	var rem uint256.Int
	rem.MulMod(x, y, denominator)
	if !rem.IsZero() {
		res.AddUint64(res, 1)
	}
	return res
}

// ceilDiv computes ceil(a/b).
func ceilDiv(a, b *uint256.Int) *uint256.Int {
	var q, rem uint256.Int
	q.DivMod(a, b, &rem)
	if !rem.IsZero() {
		q.AddUint64(&q, 1)
	}
	return &q
}

// isqrt computes floor(sqrt(x)). Matches OpenZeppelin Math.sqrt.
func isqrt(x *uint256.Int) *uint256.Int {
	return new(uint256.Int).Sqrt(x)
}
