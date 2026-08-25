package lunarbasepmm

import (
	"math/big"
	"math/rand"
	"testing"

	"github.com/holiman/uint256"
	"github.com/stretchr/testify/require"
)

var (
	oracleQ24     = new(big.Int).Lsh(big.NewInt(1), 24)
	oracleQ96     = new(big.Int).Lsh(big.NewInt(1), 96)
	oracleMaxU256 = new(big.Int).Sub(new(big.Int).Lsh(big.NewInt(1), 256), big.NewInt(1))
)

func oracleXValueInY(amount, anchor *uint256.Int) *big.Int {
	first := new(big.Int).Mul(amount.ToBig(), anchor.ToBig())
	first.Quo(first, oracleQ96)
	second := new(big.Int).Mul(first, anchor.ToBig())
	return second.Quo(second, oracleQ96)
}

func oracleYValueInX(amount, anchor *uint256.Int) *big.Int {
	if anchor.IsZero() {
		return new(big.Int)
	}
	first := new(big.Int).Mul(amount.ToBig(), oracleQ96)
	first.Quo(first, anchor.ToBig())
	second := new(big.Int).Mul(first, oracleQ96)
	return second.Quo(second, anchor.ToBig())
}

func oracleApplyFee(gross *big.Int, feeQ24 uint32, multiplier *uint256.Int) (amountOut, fee *big.Int) {
	if feeQ24 == MaxUint24 {
		return new(big.Int), new(big.Int).Set(gross)
	}
	baseFee := new(big.Int).Mul(gross, new(big.Int).SetUint64(uint64(feeQ24)))
	baseFee.Quo(baseFee, oracleQ24)
	if multiplier.Cmp(uint256.NewInt(1)) <= 0 || baseFee.Sign() == 0 {
		return new(big.Int).Sub(gross, baseFee), baseFee
	}
	multiplierBig := multiplier.ToBig()
	maxMultiplier := new(big.Int).Quo(new(big.Int).Set(oracleMaxU256), baseFee)
	if multiplierBig.Cmp(maxMultiplier) > 0 {
		return new(big.Int), new(big.Int).Set(gross)
	}
	scaledFee := new(big.Int).Mul(baseFee, multiplierBig)
	if scaledFee.Cmp(gross) >= 0 {
		return new(big.Int), new(big.Int).Set(gross)
	}
	return new(big.Int).Sub(gross, scaledFee), scaledFee
}

func oracleEffectiveFee(storedFeeX24, desiredPunishmentX24 uint32) uint32 {
	widened := uint64(storedFeeX24) + uint64(desiredPunishmentX24)
	if widened >= uint64(MaxUint24) {
		return MaxUint24
	}
	return uint32(widened)
}

func assertU256EqualsBig(t *testing.T, expected *big.Int, actual *uint256.Int) {
	t.Helper()
	if expected.Cmp(actual.ToBig()) != 0 {
		t.Fatalf("uint256 mismatch: expected %s, got %s", expected, actual.Dec())
	}
}

func randomAnchor(rng *rand.Rand, iteration int) *uint256.Int {
	if iteration%97 == 0 {
		return new(uint256.Int)
	}
	numerator := uint256.NewInt(uint64(rng.Intn(16) + 1))
	denominator := uint256.NewInt(uint64(rng.Intn(16) + 1))
	return new(uint256.Int).Div(new(uint256.Int).Mul(q96, numerator), denominator)
}

func randomMultiplier(rng *rand.Rand, iteration int) *uint256.Int {
	switch iteration % 9 {
	case 0:
		return new(uint256.Int)
	case 1:
		return uint256.NewInt(1)
	case 2:
		return new(uint256.Int).Not(new(uint256.Int))
	default:
		return uint256.NewInt(uint64(rng.Intn(1_000_000) + 2))
	}
}

func TestPropertyQuotesMatchBigIntOracle(t *testing.T) {
	rng := rand.New(rand.NewSource(0x5eedc0de))
	for i := 0; i < 2_000; i++ {
		anchor := randomAnchor(rng, i)
		amount := uint256.NewInt(rng.Uint64() % 1_000_000_000_000)
		reserveX := uint256.NewInt(rng.Uint64() % 1_000_000_000_000_000)
		reserveY := uint256.NewInt(rng.Uint64() % 1_000_000_000_000_000)
		feeAsk := uint32(rng.Uint64() % uint64(Q24Scale))
		feeBid := uint32(rng.Uint64() % uint64(Q24Scale))
		multiplier := randomMultiplier(rng, i)
		params := &PoolParams{
			SqrtPriceX96:     anchor,
			FeeAskX24:        feeAsk,
			FeeBidX24:        feeBid,
			ReserveX:         reserveX,
			ReserveY:         reserveY,
			MaxPunishmentX24: uint32(rng.Uint64() % uint64(Q24Scale)),
		}

		xResult, err := QuoteXToYWithMultiplierChecked(params, amount, multiplier)
		require.NoErrorf(t, err, "iteration %d", i)
		xGross := oracleXValueInY(amount, anchor)
		xDesired := oracleDesiredPunishment(params, amount, DirectionXToY)
		xEffectiveFee := oracleEffectiveFee(feeBid, xDesired)
		var expectedXOut, expectedXFee *big.Int
		if xGross.Sign() == 0 || xGross.Cmp(reserveY.ToBig()) > 0 {
			expectedXOut, expectedXFee = new(big.Int), new(big.Int)
		} else {
			expectedXOut, expectedXFee = oracleApplyFee(xGross, xEffectiveFee, multiplier)
		}
		assertU256EqualsBig(t, expectedXOut, xResult.AmountOut)
		assertU256EqualsBig(t, expectedXFee, xResult.Fee)
		require.Equal(t, xEffectiveFee, xResult.EffectiveFeeX24)
		require.Equal(t, anchor, xResult.SqrtPriceNext, "iteration %d", i)

		yResult, err := QuoteYToXWithMultiplierChecked(params, amount, multiplier)
		require.NoErrorf(t, err, "iteration %d", i)
		yGross := oracleYValueInX(amount, anchor)
		yDesired := oracleDesiredPunishment(params, amount, DirectionYToX)
		yEffectiveFee := oracleEffectiveFee(feeAsk, yDesired)
		var expectedYOut, expectedYFee *big.Int
		if yGross.Sign() == 0 || yGross.Cmp(reserveX.ToBig()) > 0 {
			expectedYOut, expectedYFee = new(big.Int), new(big.Int)
		} else {
			expectedYOut, expectedYFee = oracleApplyFee(yGross, yEffectiveFee, multiplier)
		}
		assertU256EqualsBig(t, expectedYOut, yResult.AmountOut)
		assertU256EqualsBig(t, expectedYFee, yResult.Fee)
		require.Equal(t, yEffectiveFee, yResult.EffectiveFeeX24)
		require.Equal(t, anchor, yResult.SqrtPriceNext, "iteration %d", i)
	}
}

func oracleDesiredPunishment(params *PoolParams, amountIn *uint256.Int, direction SwapDirection) uint32 {
	if amountIn.IsZero() || params.MaxPunishmentX24 == 0 || params.SqrtPriceX96.IsZero() {
		return 0
	}
	inventory := oracleXValueInY(params.ReserveX, params.SqrtPriceX96)
	inventory.Add(inventory, params.ReserveY.ToBig())
	if inventory.Sign() == 0 {
		return 0
	}
	swapWealth := amountIn.ToBig()
	if direction == DirectionXToY {
		swapWealth = oracleXValueInY(amountIn, params.SqrtPriceX96)
	}
	if swapWealth.Sign() == 0 {
		return 0
	}
	maximum := new(big.Int).SetUint64(uint64(params.MaxPunishmentX24))
	if params.MaxPunishmentX24 == MaxUint24 {
		maximum.Set(oracleQ24)
	}
	calculated := new(big.Int).Set(maximum)
	if swapWealth.Cmp(inventory) < 0 {
		calculated.Mul(maximum, swapWealth)
		calculated.Add(calculated, new(big.Int).Sub(inventory, big.NewInt(1)))
		calculated.Quo(calculated, inventory)
	}
	if calculated.Cmp(oracleQ24) >= 0 {
		return MaxUint24
	}
	return uint32(calculated.Uint64())
}

func TestPropertyDesiredPunishmentMatchesBigIntOracle(t *testing.T) {
	rng := rand.New(rand.NewSource(0x51a7e))
	for i := 0; i < 2_000; i++ {
		maximum := uint32(rng.Uint64() % uint64(Q24Scale))
		if i%13 == 0 {
			maximum = MaxUint24
		}
		params := &PoolParams{
			SqrtPriceX96:     randomAnchor(rng, i),
			FeeAskX24:        uint32(rng.Uint64() % uint64(Q24Scale)),
			FeeBidX24:        uint32(rng.Uint64() % uint64(Q24Scale)),
			ReserveX:         uint256.NewInt(rng.Uint64() % 1_000_000_000_000),
			ReserveY:         uint256.NewInt(rng.Uint64() % 1_000_000_000_000),
			MaxPunishmentX24: maximum,
		}
		amount := uint256.NewInt(rng.Uint64() % 1_000_000_000_000)
		direction := SwapDirection(i & 1)

		actual, err := DesiredPunishmentX24(params, amount, direction)
		require.NoErrorf(t, err, "iteration %d", i)
		require.Equal(t, oracleDesiredPunishment(params, amount, direction), actual, "iteration %d", i)
	}
}

func TestPropertyDirectionalTransitionIsSaturatingAndOneSided(t *testing.T) {
	rng := rand.New(rand.NewSource(0xd1ec710))
	for i := 0; i < 2_000; i++ {
		ask := uint32(rng.Uint64() % uint64(Q24Scale))
		bid := uint32(rng.Uint64() % uint64(Q24Scale))
		desired := uint32(rng.Uint64() % uint64(Q24Scale))
		direction := SwapDirection(i & 1)
		nextAsk, nextBid, applied, err := TransitionDirectionalFees(ask, bid, desired, direction)
		require.NoErrorf(t, err, "iteration %d", i)
		if direction == DirectionXToY {
			require.Equal(t, ask, nextAsk)
			require.Equal(t, min(desired, MaxUint24-bid), applied)
			require.Equal(t, bid+applied, nextBid)
		} else {
			require.Equal(t, bid, nextBid)
			require.Equal(t, min(desired, MaxUint24-ask), applied)
			require.Equal(t, ask+applied, nextAsk)
		}
	}
}
