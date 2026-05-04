package lunarbasepmm

import (
	"testing"

	"github.com/holiman/uint256"
	"github.com/stretchr/testify/assert"
)

func u(s string) *uint256.Int {
	v, err := uint256.FromDecimal(s)
	if err != nil {
		panic(err)
	}
	return v
}

func TestIsqrt(t *testing.T) {
	cases := []struct{ in, expected uint64 }{
		{0, 0}, {1, 1}, {4, 2}, {9, 3}, {10, 3}, {100, 10},
	}
	for _, tc := range cases {
		var got uint256.Int
		isqrt(&got, uint256.NewInt(tc.in))
		assert.Equal(t, uint256.NewInt(tc.expected), &got, "isqrt(%d)", tc.in)
	}
}

func TestConcentrationQ48_ZeroFee(t *testing.T) {
	var c uint256.Int
	concentrationQ48(&c, uint256.NewInt(1<<48), 0, uint256.NewInt(1000),
		uint256.NewInt(10000), uint256.NewInt(10000), 5000, true)
	assert.True(t, c.IsZero())
}

func TestConcentrationQ48_ZeroAmount(t *testing.T) {
	var c uint256.Int
	concentrationQ48(&c, uint256.NewInt(1<<48), 1000, new(uint256.Int),
		uint256.NewInt(10000), uint256.NewInt(10000), 5000, true)
	assert.Equal(t, uint256.NewInt(1000), &c)
}

func TestQuoteReturnsZeroWhenNoLiquidity(t *testing.T) {
	p := uint256.NewInt(1 << 48)
	params := &PoolParams{
		SqrtPriceX48:       p,
		AnchorSqrtPriceX48: p,
		FeeQ48:             1 << 44,
		ReserveX:           new(uint256.Int),
		ReserveY:           new(uint256.Int),
		ConcentrationK:     5000,
	}
	result := QuoteXToY(params, uint256.NewInt(1000))
	assert.True(t, result.AmountOut.IsZero())
}

func TestMulDivCeil(t *testing.T) {
	mu := func(x, y, d uint64) *uint256.Int {
		var dst uint256.Int
		return mulDivUp(&dst, uint256.NewInt(x), uint256.NewInt(y), uint256.NewInt(d))
	}
	assert.Equal(t, uint256.NewInt(1), mu(1, 1, 2))
	assert.Equal(t, uint256.NewInt(2), mu(3, 3, 5))
	assert.Equal(t, uint256.NewInt(2), mu(2, 2, 2))
}
