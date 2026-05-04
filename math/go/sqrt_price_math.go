package lunarbasepmm

import "github.com/holiman/uint256"

// getNextSqrtPriceFromAmountXRoundingUp ports the addX=true branch of
// Uniswap V3 SqrtPriceMath, used by quoteXToY. Writes into dst, returns dst.
func getNextSqrtPriceFromAmountXRoundingUp(dst, sqrtPX48, liquidity, amountX *uint256.Int) *uint256.Int {
	if amountX.IsZero() {
		return dst.Set(sqrtPX48)
	}

	var num1, prod, tmp uint256.Int
	num1.Lsh(liquidity, fixedPoint48Resolution)
	prod.Mul(amountX, sqrtPX48)

	if quotient := tmp.Div(&prod, amountX); quotient.Eq(sqrtPX48) {
		var deno uint256.Int
		deno.Add(&num1, &prod)
		if !deno.Lt(&num1) {
			return mulDivUp(dst, &num1, sqrtPX48, &deno)
		}
	}

	var divResult, deno uint256.Int
	divResult.Div(&num1, sqrtPX48)
	deno.Add(&divResult, amountX)
	return ceilDiv(dst, &num1, &deno)
}

// getNextSqrtPriceFromAmountYRoundingDown ports the addY=true branch of
// Uniswap V3 SqrtPriceMath, used by quoteYToX. Writes into dst, returns dst.
func getNextSqrtPriceFromAmountYRoundingDown(dst, sqrtPX48, liquidity, amountY *uint256.Int) *uint256.Int {
	var quotient uint256.Int
	if amountY.Lt(u2Pow80) {
		var shifted uint256.Int
		shifted.Lsh(amountY, fixedPoint48Resolution)
		quotient.Div(&shifted, liquidity)
	} else {
		mulDivDown(&quotient, amountY, q48, liquidity)
	}

	return dst.Add(sqrtPX48, &quotient)
}

// getAmountXDelta writes |Δx| between two sqrt prices for a given liquidity
// into dst.
func getAmountXDelta(dst, sqrtRatioA, sqrtRatioB, liquidity *uint256.Int, roundUp bool) *uint256.Int {
	sa, sb := sqrtRatioA, sqrtRatioB
	if sa.Gt(sb) {
		sa, sb = sb, sa
	}

	var num1, num2 uint256.Int
	num1.Lsh(liquidity, fixedPoint48Resolution)
	num2.Sub(sb, sa)

	if roundUp {
		mulDivUp(dst, &num1, &num2, sb)
		return ceilDiv(dst, dst, sa)
	}
	mulDivDown(dst, &num1, &num2, sb)
	return dst.Div(dst, sa)
}

// getAmountYDelta writes |Δy| between two sqrt prices for a given liquidity
// into dst.
func getAmountYDelta(dst, sqrtRatioA, sqrtRatioB, liquidity *uint256.Int, roundUp bool) *uint256.Int {
	sa, sb := sqrtRatioA, sqrtRatioB
	if sa.Gt(sb) {
		sa, sb = sb, sa
	}

	var diff uint256.Int
	diff.Sub(sb, sa)

	if roundUp {
		return mulDivUp(dst, liquidity, &diff, q48)
	}
	return mulDivDown(dst, liquidity, &diff, q48)
}
