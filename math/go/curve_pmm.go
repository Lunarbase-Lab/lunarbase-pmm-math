// Package lunarbasepmm is a pure-Go port of the lunarbase-pmm-math Rust crate.
// It implements quoteXToY / quoteYToX exactly matching the on-chain CurvePMM
// reference implementation. The package has no external dependencies beyond
// github.com/holiman/uint256 and is a direct mirror of the Rust public API.
package lunarbasepmm

import "github.com/holiman/uint256"

// PoolParams is the input snapshot needed to quote a swap.
//
// All fixed-point values use Q48 (48 fractional bits). SqrtPriceX48 and
// AnchorSqrtPriceX48 are uint80, FeeQ48 is uint48, ReserveX/ReserveY are
// uint112; the wider uint256 type is used here purely for arithmetic.
type PoolParams struct {
	SqrtPriceX48       *uint256.Int
	AnchorSqrtPriceX48 *uint256.Int
	FeeQ48             uint64
	ReserveX           *uint256.Int
	ReserveY           *uint256.Int
	ConcentrationK     uint32
}

// QuoteResult holds the output of QuoteXToY / QuoteYToX.
//
// AmountOut is net of Fee. SqrtPriceNext is the post-swap sqrt price. When the
// swap is rejected (insufficient bound, zero liquidity, etc.) AmountOut and
// Fee are zero and SqrtPriceNext equals the input SqrtPriceX48.
type QuoteResult struct {
	AmountOut     *uint256.Int
	SqrtPriceNext *uint256.Int
	Fee           *uint256.Int
}

// concentrationQ48 mirrors SwapLib.concentrationQ48: C = fee * (1 + k * r^2),
// where r is normalized by total wealth, not by raw input reserve.
func concentrationQ48(
	pX48 *uint256.Int,
	baseFeeQ48 uint64,
	amountIn *uint256.Int,
	reserveX, reserveY *uint256.Int,
	k uint32,
	xToY bool,
) *uint256.Int {
	c := uint256.NewInt(baseFeeQ48)
	if c.IsZero() || amountIn.IsZero() || k == 0 || pX48.IsZero() {
		return c
	}

	var priceQ96, q96 uint256.Int
	priceQ96.Mul(pX48, pX48)
	q96.Mul(q48, q48)

	xWealthInY := mulDivDown(reserveX, &priceQ96, &q96)

	var totalWealthInY uint256.Int
	totalWealthInY.Add(xWealthInY, reserveY)
	if totalWealthInY.IsZero() {
		return c
	}

	var amountInWealth *uint256.Int
	if xToY {
		amountInWealth = mulDivDown(amountIn, &priceQ96, &q96)
	} else {
		amountInWealth = new(uint256.Int).Set(amountIn)
	}

	var rQ48 uint256.Int
	if !amountInWealth.Lt(&totalWealthInY) {
		rQ48.Set(q48)
	} else {
		rQ48.Set(mulDivDown(amountInWealth, q48, &totalWealthInY))
	}

	rSquaredQ48 := mulDivDown(&rQ48, &rQ48, q48)

	kU := uint256.NewInt(uint64(k))
	var kTimesR2, multiplierQ48 uint256.Int
	kTimesR2.Mul(kU, rSquaredQ48)
	multiplierQ48.Add(q48, &kTimesR2)

	result := mulDivDown(c, &multiplierQ48, q48)

	if !result.Lt(q48) {
		return new(uint256.Int).Set(q48)
	}
	return result
}

// lowerBound = pX48 * sqrt(1 - C) (Q48-fixed-point arithmetic).
func lowerBound(pX48 *uint256.Int, cQ48 uint64) *uint256.Int {
	var oneMinusC uint256.Int
	oneMinusC.Sub(q48, oneMinusC.SetUint64(cQ48))
	return mulDivDown(pX48, isqrt(&oneMinusC), q24)
}

// upperBound = pX48 / sqrt(1 - C) (Q48-fixed-point arithmetic).
func upperBound(pX48 *uint256.Int, cQ48 uint64) *uint256.Int {
	var oneMinusC uint256.Int
	oneMinusC.Sub(q48, oneMinusC.SetUint64(cQ48))
	return mulDivDown(pX48, q24, isqrt(&oneMinusC))
}

// liquidityY = reserveY * Q48 / (pX48 - pBid).
func liquidityY(pX48, pBid, reserveY *uint256.Int) *uint256.Int {
	var denom uint256.Int
	denom.Sub(pX48, pBid)
	return mulDivDown(reserveY, q48, &denom)
}

// liquidityX = reserveX * (pX48 * pAsk) / (Q48 * (pAsk - pX48)).
func liquidityX(pX48, pAsk, reserveX *uint256.Int) *uint256.Int {
	var num uint256.Int
	num.Mul(pX48, pAsk)

	var denom uint256.Int
	denom.Sub(pAsk, pX48)
	denom.Mul(q48, &denom)

	return mulDivDown(reserveX, &num, &denom)
}

// QuoteXToY is an exact port of CurvePMM.quoteXToY.
//
// Returns a non-nil QuoteResult with zero amount/fee and unchanged sqrtPrice
// when the swap would be rejected (concentration saturates, no bid liquidity,
// or input exceeds the maximum net Δx).
func QuoteXToY(params *PoolParams, dx *uint256.Int) *QuoteResult {
	zero := &QuoteResult{
		AmountOut:     new(uint256.Int),
		SqrtPriceNext: new(uint256.Int).Set(params.SqrtPriceX48),
		Fee:           new(uint256.Int),
	}

	cQ48 := concentrationQ48(
		params.SqrtPriceX48,
		params.FeeQ48,
		dx,
		params.ReserveX,
		params.ReserveY,
		params.ConcentrationK,
		true,
	)
	if !cQ48.Lt(q48) {
		return zero
	}

	cU64 := cQ48.Uint64()
	pBid := lowerBound(params.AnchorSqrtPriceX48, cU64)
	if !params.SqrtPriceX48.Gt(pBid) {
		return zero
	}
	liquidity := liquidityY(params.SqrtPriceX48, pBid, params.ReserveY)

	maxNetDx := getAmountXDelta(pBid, params.SqrtPriceX48, liquidity, false)
	if dx.Gt(maxNetDx) {
		return zero
	}

	pNext := getNextSqrtPriceFromAmountXRoundingUp(params.SqrtPriceX48, liquidity, dx)
	dy := getAmountYDelta(params.SqrtPriceX48, pNext, liquidity, false)

	fee := mulDivDown(dy, uint256.NewInt(params.FeeQ48), q48)
	dyAfterFee := new(uint256.Int).Sub(dy, fee)

	return &QuoteResult{
		AmountOut:     dyAfterFee,
		SqrtPriceNext: pNext,
		Fee:           fee,
	}
}

// QuoteYToX is an exact port of CurvePMM.quoteYToX. See QuoteXToY for the
// rejection semantics.
func QuoteYToX(params *PoolParams, dy *uint256.Int) *QuoteResult {
	zero := &QuoteResult{
		AmountOut:     new(uint256.Int),
		SqrtPriceNext: new(uint256.Int).Set(params.SqrtPriceX48),
		Fee:           new(uint256.Int),
	}

	cQ48 := concentrationQ48(
		params.SqrtPriceX48,
		params.FeeQ48,
		dy,
		params.ReserveX,
		params.ReserveY,
		params.ConcentrationK,
		false,
	)
	if !cQ48.Lt(q48) {
		return zero
	}

	cU64 := cQ48.Uint64()
	pAsk := upperBound(params.AnchorSqrtPriceX48, cU64)
	if !params.SqrtPriceX48.Lt(pAsk) {
		return zero
	}
	liquidity := liquidityX(params.SqrtPriceX48, pAsk, params.ReserveX)

	maxNetDy := getAmountYDelta(params.SqrtPriceX48, pAsk, liquidity, false)
	if dy.Gt(maxNetDy) {
		return zero
	}

	pNext := getNextSqrtPriceFromAmountYRoundingDown(params.SqrtPriceX48, liquidity, dy)
	dxOut := getAmountXDelta(params.SqrtPriceX48, pNext, liquidity, false)

	fee := mulDivDown(dxOut, uint256.NewInt(params.FeeQ48), q48)
	dxAfterFee := new(uint256.Int).Sub(dxOut, fee)

	return &QuoteResult{
		AmountOut:     dxAfterFee,
		SqrtPriceNext: pNext,
		Fee:           fee,
	}
}
