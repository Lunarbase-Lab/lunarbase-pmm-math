// Package lunarbasepmm is a pure-Go port of the lunarbase-pmm-math Rust crate.
// It implements quoteXToY / quoteYToX exactly matching the on-chain `SwapLib`
// reference on the `fix/incident` branch (single-price Q32.48 design).
// No external dependencies beyond github.com/holiman/uint256.
package lunarbasepmm

import "github.com/holiman/uint256"

// PoolParams is the input snapshot needed to quote a swap.
//
// Widths follow the on-chain contract: SqrtPriceX48 is uint80 (Q32.48),
// FeeAskX24 / FeeBidX24 are uint24 (Q24, where Q24 represents 100%),
// ReserveX / ReserveY are uint112, ConcentrationK is uint32 stored as
// Q20.12 (effective K = ConcentrationK / 2^12).
//
// SqrtPriceX48 is the single canonical price (operator-set; swaps do not
// mutate it). There is no separate live-vs-anchor split.
type PoolParams struct {
	SqrtPriceX48   *uint256.Int
	FeeAskX24      uint32
	FeeBidX24      uint32
	ReserveX       *uint256.Int
	ReserveY       *uint256.Int
	ConcentrationK uint32
}

// QuoteResult holds the output of QuoteXToY / QuoteYToX.
//
// AmountOut is net of Fee. SqrtPriceNext is the hypothetical post-swap
// Q32.48 sqrt price (informational — pool storage is unchanged by a swap on
// the fix/incident design). When the swap is rejected, AmountOut and Fee are
// zero and SqrtPriceNext equals the input SqrtPriceX48.
type QuoteResult struct {
	AmountOut     *uint256.Int
	SqrtPriceNext *uint256.Int
	Fee           *uint256.Int
}

// concentrationQ48 writes c = mulDiv(concentrationK, r², Q12) into dst,
// where r is wealth-normalised by cascading `mulDiv(_, sqrtP_Q48, Q48)`
// twice to compute `reserveX * P` precisely. Saturates at Q48 (100%).
//
// Returns dst zeroed when amountIn, k, or sqrtPriceX48 is zero — that
// triggers the linear-fallback path in the callers.
//
// Mirrors Solidity `SwapLib.concentrationQ48` bit-for-bit.
func concentrationQ48(
	dst, sqrtPriceX48 *uint256.Int,
	amountIn *uint256.Int,
	reserveX, reserveY *uint256.Int,
	kQ12 uint32,
	xToY bool,
) *uint256.Int {
	if amountIn.IsZero() || kQ12 == 0 || sqrtPriceX48.IsZero() {
		dst.Clear()
		return dst
	}

	// xWealthInY = mulDiv(mulDiv(reserveX, sqrtPX48, Q48), sqrtPX48, Q48)
	//            = reserveX * P (where P = sqrtP^2)
	var xWealthInY, totalWealthInY, scratch uint256.Int
	mulDivDown(&scratch, reserveX, sqrtPriceX48, q48)
	mulDivDown(&xWealthInY, &scratch, sqrtPriceX48, q48)
	totalWealthInY.Add(&xWealthInY, reserveY)
	if totalWealthInY.IsZero() {
		dst.Clear()
		return dst
	}

	var amountInWealth uint256.Int
	if xToY {
		mulDivDown(&scratch, amountIn, sqrtPriceX48, q48)
		mulDivDown(&amountInWealth, &scratch, sqrtPriceX48, q48)
	} else {
		amountInWealth.Set(amountIn)
	}

	// r in Q48: min(amountInWealth / totalWealth, 1) * Q48.
	var rQ48 uint256.Int
	if !amountInWealth.Lt(&totalWealthInY) {
		rQ48.Set(q48)
	} else {
		mulDivDown(&rQ48, &amountInWealth, q48, &totalWealthInY)
	}

	// r² in Q48.
	var rSquaredQ48 uint256.Int
	mulDivDown(&rSquaredQ48, &rQ48, &rQ48, q48)

	// c = mulDiv(K_Q12, r², Q12). Saturate at Q48.
	var kU uint256.Int
	kU.SetUint64(uint64(kQ12))
	mulDivDown(dst, &kU, &rSquaredQ48, q12)
	if !dst.Lt(q48) {
		dst.Set(q48)
	}
	return dst
}

// lowerBound writes sqrtPriceX48 * sqrt(1 - C) (Q32.48) into dst.
func lowerBound(dst, sqrtPriceX48 *uint256.Int, cQ48 uint64) *uint256.Int {
	var oneMinusC, sqrtOneMinusC uint256.Int
	oneMinusC.Sub(q48, oneMinusC.SetUint64(cQ48))
	isqrt(&sqrtOneMinusC, &oneMinusC)
	return mulDivDown(dst, sqrtPriceX48, &sqrtOneMinusC, q24)
}

// upperBound writes sqrtPriceX48 / sqrt(1 - C) (Q32.48) into dst.
func upperBound(dst, sqrtPriceX48 *uint256.Int, cQ48 uint64) *uint256.Int {
	var oneMinusC, sqrtOneMinusC uint256.Int
	oneMinusC.Sub(q48, oneMinusC.SetUint64(cQ48))
	isqrt(&sqrtOneMinusC, &oneMinusC)
	return mulDivDown(dst, sqrtPriceX48, q24, &sqrtOneMinusC)
}

// liquidityY writes reserveY * Q48 / (sqrtPriceX48 - pBid) into dst.
func liquidityY(dst, sqrtPriceX48, pBid, reserveY *uint256.Int) *uint256.Int {
	var denom uint256.Int
	denom.Sub(sqrtPriceX48, pBid)
	return mulDivDown(dst, reserveY, q48, &denom)
}

// liquidityX writes reserveX * (sqrtPriceX48 * pAsk) / (Q48 * (pAsk - sqrtPriceX48))
// into dst. Avoids the intermediate /Q96 truncation that bites at small
// sqrt-prices (anchor*pAsk ≈ Q96 when sqrtP ≈ 2^48). Mirrors `SwapLib.Lx`
// after the Q96 → Q48 migration.
func liquidityX(dst, sqrtPriceX48, pAsk, reserveX *uint256.Int) *uint256.Int {
	var numerator, denominator, diff uint256.Int
	numerator.Mul(sqrtPriceX48, pAsk)
	diff.Sub(pAsk, sqrtPriceX48)
	denominator.Mul(q48, &diff)
	return mulDivDown(dst, reserveX, &numerator, &denominator)
}

// QuoteXToY is an exact port of `SwapLib._quoteXToY` on the fix/incident
// branch. Allocates a fresh `QuoteResult` per call. For tight loops use
// [QuoteXToYInto].
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

// QuoteXToYInto computes the quote and writes the result into out.
// Allocation-free on the hot path. The caller owns out and its three
// `*uint256.Int` fields; all of them must be non-nil.
func QuoteXToYInto(out *QuoteResult, params *PoolParams, dx *uint256.Int) *QuoteResult {
	var (
		cQ48      uint256.Int
		pBid      uint256.Int
		liquidity uint256.Int
		maxNetDx  uint256.Int
		pNext     uint256.Int
		dy        uint256.Int
		feeQ24    uint256.Int
	)

	concentrationQ48(&cQ48, params.SqrtPriceX48, dx,
		params.ReserveX, params.ReserveY, params.ConcentrationK, true)
	if cQ48.IsZero() {
		linearXToY(out, params, dx)
		return out
	}
	if !cQ48.Lt(q48) {
		return writeRejected(out, params)
	}

	lowerBound(&pBid, params.SqrtPriceX48, cQ48.Uint64())
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

	feeQ24.SetUint64(uint64(params.FeeBidX24))
	mulDivDown(out.Fee, &dy, &feeQ24, q24)
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
		feeQ24    uint256.Int
	)

	concentrationQ48(&cQ48, params.SqrtPriceX48, dy,
		params.ReserveX, params.ReserveY, params.ConcentrationK, false)
	if cQ48.IsZero() {
		linearYToX(out, params, dy)
		return out
	}
	if !cQ48.Lt(q48) {
		return writeRejected(out, params)
	}

	upperBound(&pAsk, params.SqrtPriceX48, cQ48.Uint64())
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

	feeQ24.SetUint64(uint64(params.FeeAskX24))
	mulDivDown(out.Fee, &dxOut, &feeQ24, q24)
	out.AmountOut.Sub(&dxOut, out.Fee)
	out.SqrtPriceNext.Set(&pNext)
	return out
}

// linearXToY implements the cQ48 == 0 fallback for X → Y:
// dy = mulDiv(mulDiv(dx, sqrtPriceX48, Q48), sqrtPriceX48, Q48),
// fee on dy, pNext = sqrtPriceX48.
func linearXToY(out *QuoteResult, params *PoolParams, dx *uint256.Int) {
	var dyGross, scratch, feeQ24 uint256.Int
	mulDivDown(&scratch, dx, params.SqrtPriceX48, q48)
	mulDivDown(&dyGross, &scratch, params.SqrtPriceX48, q48)
	if dyGross.IsZero() || dyGross.Gt(params.ReserveY) {
		writeRejected(out, params)
		return
	}

	feeQ24.SetUint64(uint64(params.FeeBidX24))
	mulDivDown(out.Fee, &dyGross, &feeQ24, q24)
	out.AmountOut.Sub(&dyGross, out.Fee)
	out.SqrtPriceNext.Set(params.SqrtPriceX48)
}

// linearYToX is the cQ48 == 0 fallback for Y → X:
// dx = mulDiv(mulDiv(dy, Q48, sqrtPriceX48), Q48, sqrtPriceX48),
// fee on dx, pNext = sqrtPriceX48.
func linearYToX(out *QuoteResult, params *PoolParams, dy *uint256.Int) {
	if params.SqrtPriceX48.IsZero() {
		writeRejected(out, params)
		return
	}

	var dxGross, scratch, feeQ24 uint256.Int
	mulDivDown(&scratch, dy, q48, params.SqrtPriceX48)
	mulDivDown(&dxGross, &scratch, q48, params.SqrtPriceX48)
	if dxGross.IsZero() || dxGross.Gt(params.ReserveX) {
		writeRejected(out, params)
		return
	}

	feeQ24.SetUint64(uint64(params.FeeAskX24))
	mulDivDown(out.Fee, &dxGross, &feeQ24, q24)
	out.AmountOut.Sub(&dxGross, out.Fee)
	out.SqrtPriceNext.Set(params.SqrtPriceX48)
}

// writeRejected fills out with a zero-output result preserving the input
// sqrt-price.
func writeRejected(out *QuoteResult, params *PoolParams) *QuoteResult {
	out.AmountOut.Clear()
	out.Fee.Clear()
	out.SqrtPriceNext.Set(params.SqrtPriceX48)
	return out
}
