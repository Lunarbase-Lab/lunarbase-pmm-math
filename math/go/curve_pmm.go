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

// concentrationQ48 writes C = fee * (1 + k * r^2) into dst, where r is
// normalized by total wealth, not by raw input reserve.
func concentrationQ48(
	dst, pX48 *uint256.Int,
	baseFeeQ48 uint64,
	amountIn *uint256.Int,
	reserveX, reserveY *uint256.Int,
	k uint32,
	xToY bool,
) *uint256.Int {
	dst.SetUint64(baseFeeQ48)
	if dst.IsZero() || amountIn.IsZero() || k == 0 || pX48.IsZero() {
		return dst
	}

	var priceQ96 uint256.Int
	priceQ96.Mul(pX48, pX48)

	var xWealthInY, totalWealthInY uint256.Int
	mulDivDown(&xWealthInY, reserveX, &priceQ96, q96)
	totalWealthInY.Add(&xWealthInY, reserveY)
	if totalWealthInY.IsZero() {
		return dst
	}

	var amountInWealth uint256.Int
	if xToY {
		mulDivDown(&amountInWealth, amountIn, &priceQ96, q96)
	} else {
		amountInWealth.Set(amountIn)
	}

	// r in Q48: min(amountInWealth/totalWealth, 1) * Q48
	var rQ48 uint256.Int
	if !amountInWealth.Lt(&totalWealthInY) {
		rQ48.Set(q48)
	} else {
		mulDivDown(&rQ48, &amountInWealth, q48, &totalWealthInY)
	}

	// r^2 in Q48
	var rSquaredQ48 uint256.Int
	mulDivDown(&rSquaredQ48, &rQ48, &rQ48, q48)

	// multiplier = Q48 + k * r^2
	var multiplierQ48, kTimesR2, kU uint256.Int
	kU.SetUint64(uint64(k))
	kTimesR2.Mul(&kU, &rSquaredQ48)
	multiplierQ48.Add(q48, &kTimesR2)

	// C = fee * multiplier / Q48, capped at Q48.
	mulDivDown(dst, dst, &multiplierQ48, q48)
	if !dst.Lt(q48) {
		dst.Set(q48)
	}
	return dst
}

// lowerBound writes pX48 * sqrt(1 - C) (Q48) into dst.
func lowerBound(dst, pX48 *uint256.Int, cQ48 uint64) *uint256.Int {
	var oneMinusC, sqrtOneMinusC uint256.Int
	oneMinusC.Sub(q48, oneMinusC.SetUint64(cQ48))
	isqrt(&sqrtOneMinusC, &oneMinusC)
	return mulDivDown(dst, pX48, &sqrtOneMinusC, q24)
}

// upperBound writes pX48 / sqrt(1 - C) (Q48) into dst.
func upperBound(dst, pX48 *uint256.Int, cQ48 uint64) *uint256.Int {
	var oneMinusC, sqrtOneMinusC uint256.Int
	oneMinusC.Sub(q48, oneMinusC.SetUint64(cQ48))
	isqrt(&sqrtOneMinusC, &oneMinusC)
	return mulDivDown(dst, pX48, q24, &sqrtOneMinusC)
}

// liquidityY writes reserveY * Q48 / (pX48 - pBid) into dst.
func liquidityY(dst, pX48, pBid, reserveY *uint256.Int) *uint256.Int {
	var denom uint256.Int
	denom.Sub(pX48, pBid)
	return mulDivDown(dst, reserveY, q48, &denom)
}

// liquidityX writes reserveX * (pX48 * pAsk) / (Q48 * (pAsk - pX48)) into dst.
func liquidityX(dst, pX48, pAsk, reserveX *uint256.Int) *uint256.Int {
	var num, denom uint256.Int
	num.Mul(pX48, pAsk)
	denom.Sub(pAsk, pX48)
	denom.Mul(q48, &denom)
	return mulDivDown(dst, reserveX, &num, &denom)
}

// QuoteXToY is an exact port of CurvePMM.quoteXToY.
//
// Returns a non-nil QuoteResult with zero amount/fee and unchanged sqrtPrice
// when the swap would be rejected (concentration saturates, no bid liquidity,
// or input exceeds the maximum net Δx).
//
// This wrapper allocates a fresh QuoteResult on every call. To amortize that
// allocation across many quotes, use [QuoteXToYInto] with a caller-owned
// buffer.
func QuoteXToY(params *PoolParams, dx *uint256.Int) *QuoteResult {
	out := &QuoteResult{
		AmountOut:     new(uint256.Int),
		SqrtPriceNext: new(uint256.Int),
		Fee:           new(uint256.Int),
	}
	QuoteXToYInto(out, params, dx)
	return out
}

// QuoteYToX mirrors [QuoteXToY] for the reverse direction.
func QuoteYToX(params *PoolParams, dy *uint256.Int) *QuoteResult {
	out := &QuoteResult{
		AmountOut:     new(uint256.Int),
		SqrtPriceNext: new(uint256.Int),
		Fee:           new(uint256.Int),
	}
	QuoteYToXInto(out, params, dy)
	return out
}

// QuoteXToYInto computes the quote and writes the result into out. The caller
// owns out and its three *uint256.Int fields; all of them must be non-nil.
// The hot path allocates nothing — useful for tight loops (e.g. routing or
// fuzz harnesses) where allocation pressure matters.
//
// Returns out for chaining.
func QuoteXToYInto(out *QuoteResult, params *PoolParams, dx *uint256.Int) *QuoteResult {
	var (
		cQ48      uint256.Int
		pBid      uint256.Int
		liquidity uint256.Int
		maxNetDx  uint256.Int
		pNext     uint256.Int
		dy        uint256.Int
		feeQ48    uint256.Int
	)

	concentrationQ48(&cQ48, params.SqrtPriceX48, params.FeeQ48, dx,
		params.ReserveX, params.ReserveY, params.ConcentrationK, true)
	if !cQ48.Lt(q48) {
		return writeRejected(out, params)
	}

	lowerBound(&pBid, params.AnchorSqrtPriceX48, cQ48.Uint64())
	if !params.SqrtPriceX48.Gt(&pBid) {
		return writeRejected(out, params)
	}
	liquidityY(&liquidity, params.SqrtPriceX48, &pBid, params.ReserveY)

	getAmountXDelta(&maxNetDx, &pBid, params.SqrtPriceX48, &liquidity, false)
	if dx.Gt(&maxNetDx) {
		return writeRejected(out, params)
	}

	getNextSqrtPriceFromAmountXRoundingUp(&pNext, params.SqrtPriceX48, &liquidity, dx)
	getAmountYDelta(&dy, params.SqrtPriceX48, &pNext, &liquidity, false)

	feeQ48.SetUint64(params.FeeQ48)
	mulDivDown(out.Fee, &dy, &feeQ48, q48)
	out.AmountOut.Sub(&dy, out.Fee)
	out.SqrtPriceNext.Set(&pNext)
	return out
}

// QuoteYToXInto mirrors [QuoteXToYInto] for the reverse direction.
func QuoteYToXInto(out *QuoteResult, params *PoolParams, dy *uint256.Int) *QuoteResult {
	var (
		cQ48      uint256.Int
		pAsk      uint256.Int
		liquidity uint256.Int
		maxNetDy  uint256.Int
		pNext     uint256.Int
		dxOut     uint256.Int
		feeQ48    uint256.Int
	)

	concentrationQ48(&cQ48, params.SqrtPriceX48, params.FeeQ48, dy,
		params.ReserveX, params.ReserveY, params.ConcentrationK, false)
	if !cQ48.Lt(q48) {
		return writeRejected(out, params)
	}

	upperBound(&pAsk, params.AnchorSqrtPriceX48, cQ48.Uint64())
	if !params.SqrtPriceX48.Lt(&pAsk) {
		return writeRejected(out, params)
	}
	liquidityX(&liquidity, params.SqrtPriceX48, &pAsk, params.ReserveX)

	getAmountYDelta(&maxNetDy, params.SqrtPriceX48, &pAsk, &liquidity, false)
	if dy.Gt(&maxNetDy) {
		return writeRejected(out, params)
	}

	getNextSqrtPriceFromAmountYRoundingDown(&pNext, params.SqrtPriceX48, &liquidity, dy)
	getAmountXDelta(&dxOut, params.SqrtPriceX48, &pNext, &liquidity, false)

	feeQ48.SetUint64(params.FeeQ48)
	mulDivDown(out.Fee, &dxOut, &feeQ48, q48)
	out.AmountOut.Sub(&dxOut, out.Fee)
	out.SqrtPriceNext.Set(&pNext)
	return out
}

// writeRejected fills out with a zero-output result preserving the input
// sqrt-price. The caller owns out's fields, so we mutate them in place.
func writeRejected(out *QuoteResult, params *PoolParams) *QuoteResult {
	out.AmountOut.Clear()
	out.Fee.Clear()
	out.SqrtPriceNext.Set(params.SqrtPriceX48)
	return out
}
