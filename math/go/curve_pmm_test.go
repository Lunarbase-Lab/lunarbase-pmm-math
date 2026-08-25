package lunarbasepmm

import (
	"errors"
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

func testPool() *PoolParams {
	return &PoolParams{
		SqrtPriceX96:     new(uint256.Int).Set(q96),
		FeeAskX24:        0,
		FeeBidX24:        0,
		ReserveX:         uint256.NewInt(1_000_000),
		ReserveY:         uint256.NewInt(1_000_000),
		MaxPunishmentX24: 0,
	}
}

func TestValidatePoolParamsWidths(t *testing.T) {
	max160 := new(uint256.Int).Sub(new(uint256.Int).Lsh(uint256.NewInt(1), 160), uint256.NewInt(1))
	max112 := new(uint256.Int).Sub(new(uint256.Int).Lsh(uint256.NewInt(1), 112), uint256.NewInt(1))
	valid := &PoolParams{
		SqrtPriceX96:     max160,
		FeeAskX24:        MaxUint24,
		FeeBidX24:        MaxUint24,
		ReserveX:         max112,
		ReserveY:         new(uint256.Int).Set(max112),
		MaxPunishmentX24: MaxUint24,
	}
	assert.NoError(t, ValidatePoolParams(valid))

	badAnchor := *valid
	badAnchor.SqrtPriceX96 = new(uint256.Int).Lsh(uint256.NewInt(1), 160)
	assert.ErrorIs(t, ValidatePoolParams(&badAnchor), ErrValueOutOfRange)

	badReserve := *valid
	badReserve.ReserveX = new(uint256.Int).Lsh(uint256.NewInt(1), 112)
	assert.ErrorIs(t, ValidatePoolParams(&badReserve), ErrValueOutOfRange)

	badFee := *valid
	badFee.FeeAskX24 = Q24Scale
	assert.ErrorIs(t, ValidatePoolParams(&badFee), ErrValueOutOfRange)

	badMax := *valid
	badMax.MaxPunishmentX24 = Q24Scale
	assert.ErrorIs(t, ValidatePoolParams(&badMax), ErrValueOutOfRange)
}

func TestQuoteReturnsZeroWhenNoLiquidity(t *testing.T) {
	params := testPool()
	params.ReserveX.Clear()
	params.ReserveY.Clear()
	result := QuoteXToY(params, uint256.NewInt(1000))
	assert.True(t, result.AmountOut.IsZero())
	assert.True(t, result.Fee.IsZero())
	assert.Equal(t, params.SqrtPriceX96, result.SqrtPriceNext)
}

func TestQuoteXToYUsesNestedFloors(t *testing.T) {
	params := testPool()
	params.SqrtPriceX96.Div(q96, uint256.NewInt(3))
	result := QuoteXToY(params, uint256.NewInt(19))
	// A collapsed floor(19*anchor^2/Q192) is 2; Solidity's nested floors are 1.
	assert.Equal(t, uint256.NewInt(1), result.AmountOut)
	assert.Equal(t, params.SqrtPriceX96, result.SqrtPriceNext)
}

func TestQuoteYToXUsesNestedFloors(t *testing.T) {
	params := testPool()
	params.SqrtPriceX96.Mul(q96, uint256.NewInt(3)).Rsh(params.SqrtPriceX96, 1)
	result := QuoteYToX(params, uint256.NewInt(7))
	// A collapsed floor(7*Q192/anchor^2) is 3; Solidity's nested floors are 2.
	assert.Equal(t, uint256.NewInt(2), result.AmountOut)
	assert.Equal(t, params.SqrtPriceX96, result.SqrtPriceNext)
}

func TestDirectionalFeeAndMultiplier(t *testing.T) {
	params := testPool()
	params.FeeAskX24 = Q24Scale / 10
	params.FeeBidX24 = 0

	xToY := QuoteXToY(params, uint256.NewInt(1000))
	assert.Equal(t, uint256.NewInt(1000), xToY.AmountOut)
	assert.True(t, xToY.Fee.IsZero())

	yToX := QuoteYToX(params, uint256.NewInt(1000))
	assert.Equal(t, uint256.NewInt(901), yToX.AmountOut)
	assert.Equal(t, uint256.NewInt(99), yToX.Fee)

	scaled := QuoteYToXWithMultiplier(params, uint256.NewInt(1000), uint256.NewInt(2))
	assert.Equal(t, uint256.NewInt(802), scaled.AmountOut)
	assert.Equal(t, uint256.NewInt(198), scaled.Fee)

	zeroMultiplier := QuoteYToXWithMultiplier(params, uint256.NewInt(1000), new(uint256.Int))
	assert.Equal(t, yToX.AmountOut, zeroMultiplier.AmountOut)
	assert.Equal(t, yToX.Fee, zeroMultiplier.Fee)
}

func TestFeeSentinelAndMultiplierOverflowConsumeGrossOutput(t *testing.T) {
	params := testPool()
	params.FeeBidX24 = MaxUint24
	sentinel := QuoteXToY(params, uint256.NewInt(1))
	assert.True(t, sentinel.AmountOut.IsZero())
	assert.Equal(t, uint256.NewInt(1), sentinel.Fee)

	params.FeeBidX24 = Q24Scale / 10
	max := new(uint256.Int).Not(new(uint256.Int))
	overflow := QuoteXToYWithMultiplier(params, uint256.NewInt(1_000_000), max)
	assert.True(t, overflow.AmountOut.IsZero())
	assert.Equal(t, uint256.NewInt(1_000_000), overflow.Fee)
}

func TestQuoteCheckedReportsMathOverflow(t *testing.T) {
	params := testPool()
	params.SqrtPriceX96.Sub(new(uint256.Int).Lsh(uint256.NewInt(1), 160), uint256.NewInt(1))
	params.ReserveY.Sub(new(uint256.Int).Lsh(uint256.NewInt(1), 112), uint256.NewInt(1))
	max := new(uint256.Int).Not(new(uint256.Int))
	_, err := QuoteXToYChecked(params, max)
	assert.True(t, errors.Is(err, ErrMathOverflow), err)
}

func TestQuoteIntoHotPathAllocatesNothing(t *testing.T) {
	params := testPool()
	amount := uint256.NewInt(1000)
	out := newQuoteResult()
	allocs := testing.AllocsPerRun(1000, func() {
		if err := QuoteXToYIntoChecked(out, params, amount); err != nil {
			t.Fatal(err)
		}
	})
	assert.Zero(t, allocs)
}
