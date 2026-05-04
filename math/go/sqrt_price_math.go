package lunarbasepmm

import "github.com/holiman/uint256"

// getNextSqrtPriceFromAmountXRoundingUp ports the addX=true branch of
// Uniswap V3 SqrtPriceMath, used by quoteXToY.
func getNextSqrtPriceFromAmountXRoundingUp(sqrtPX48, liquidity, amountX *uint256.Int) *uint256.Int {
	if amountX.IsZero() {
		return new(uint256.Int).Set(sqrtPX48)
	}

	var num1, prod, tmp uint256.Int
	num1.Lsh(liquidity, fixedPoint48Resolution)
	prod.Mul(amountX, sqrtPX48)

	if quotient := tmp.Div(&prod, amountX); quotient.Eq(sqrtPX48) {
		var deno uint256.Int
		deno.Add(&num1, &prod)
		if !deno.Lt(&num1) {
			return mulDivUp(&num1, sqrtPX48, &deno)
		}
	}

	var divResult uint256.Int
	divResult.Div(&num1, sqrtPX48)
	var deno uint256.Int
	deno.Add(&divResult, amountX)
	return ceilDiv(&num1, &deno)
}

// getNextSqrtPriceFromAmountYRoundingDown ports the addY=true branch of
// Uniswap V3 SqrtPriceMath, used by quoteYToX.
func getNextSqrtPriceFromAmountYRoundingDown(sqrtPX48, liquidity, amountY *uint256.Int) *uint256.Int {
	var quotient uint256.Int
	if amountY.Lt(u2Pow80) {
		shifted := new(uint256.Int).Lsh(amountY, fixedPoint48Resolution)
		quotient.Div(shifted, liquidity)
	} else {
		quotient.Set(mulDivDown(amountY, q48, liquidity))
	}

	return new(uint256.Int).Add(sqrtPX48, &quotient)
}

// getAmountXDelta returns |Δx| between two sqrt prices for a given liquidity.
func getAmountXDelta(sqrtRatioA, sqrtRatioB, liquidity *uint256.Int, roundUp bool) *uint256.Int {
	sa, sb := sqrtRatioA, sqrtRatioB
	if sa.Gt(sb) {
		sa, sb = sb, sa
	}

	var num1, num2 uint256.Int
	num1.Lsh(liquidity, fixedPoint48Resolution)
	num2.Sub(sb, sa)

	if roundUp {
		md := mulDivUp(&num1, &num2, sb)
		return ceilDiv(md, sa)
	}
	md := mulDivDown(&num1, &num2, sb)
	return md.Div(md, sa)
}

// getAmountYDelta returns |Δy| between two sqrt prices for a given liquidity.
func getAmountYDelta(sqrtRatioA, sqrtRatioB, liquidity *uint256.Int, roundUp bool) *uint256.Int {
	sa, sb := sqrtRatioA, sqrtRatioB
	if sa.Gt(sb) {
		sa, sb = sb, sa
	}

	var diff uint256.Int
	diff.Sub(sb, sa)

	if roundUp {
		return mulDivUp(liquidity, &diff, q48)
	}
	return mulDivDown(liquidity, &diff, q48)
}
