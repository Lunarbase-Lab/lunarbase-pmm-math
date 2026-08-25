package lunarbasepmm

import (
	"errors"
	"testing"

	"github.com/holiman/uint256"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func clonePool(params *PoolParams) *PoolParams {
	return &PoolParams{
		SqrtPriceX96:     new(uint256.Int).Set(params.SqrtPriceX96),
		FeeAskX24:        params.FeeAskX24,
		FeeBidX24:        params.FeeBidX24,
		ReserveX:         new(uint256.Int).Set(params.ReserveX),
		ReserveY:         new(uint256.Int).Set(params.ReserveY),
		MaxPunishmentX24: params.MaxPunishmentX24,
	}
}

func assertPoolEqual(t *testing.T, expected, actual *PoolParams) {
	t.Helper()
	assert.Equal(t, expected.SqrtPriceX96, actual.SqrtPriceX96)
	assert.Equal(t, expected.FeeAskX24, actual.FeeAskX24)
	assert.Equal(t, expected.FeeBidX24, actual.FeeBidX24)
	assert.Equal(t, expected.ReserveX, actual.ReserveX)
	assert.Equal(t, expected.ReserveY, actual.ReserveY)
	assert.Equal(t, expected.MaxPunishmentX24, actual.MaxPunishmentX24)
}

func TestDesiredPunishmentUsesPreSwapWealthAndCeil(t *testing.T) {
	params := testPool()
	params.ReserveX.SetUint64(100)
	params.ReserveY.SetUint64(100)
	params.MaxPunishmentX24 = 1000

	xToY, err := DesiredPunishmentX24(params, uint256.NewInt(100), DirectionXToY)
	require.NoError(t, err)
	assert.Equal(t, uint32(500), xToY)

	yToX, err := DesiredPunishmentX24(params, uint256.NewInt(1), DirectionYToX)
	require.NoError(t, err)
	assert.Equal(t, uint32(5), yToX)

	params.MaxPunishmentX24 = 1
	minimum, err := DesiredPunishmentX24(params, uint256.NewInt(1), DirectionXToY)
	require.NoError(t, err)
	assert.Equal(t, uint32(1), minimum, "nonzero ratio rounds up to one Q24 unit")
}

func TestDesiredPunishmentConceptualQ24Sentinel(t *testing.T) {
	params := testPool()
	params.ReserveX.SetUint64(100)
	params.ReserveY.SetUint64(200)
	params.MaxPunishmentX24 = MaxUint24

	oneThird, err := DesiredPunishmentX24(params, uint256.NewInt(100), DirectionXToY)
	require.NoError(t, err)
	assert.Equal(t, uint32(5_592_406), oneThird)

	saturated, err := DesiredPunishmentX24(params, uint256.NewInt(300), DirectionYToX)
	require.NoError(t, err)
	assert.Equal(t, MaxUint24, saturated)
}

func TestTransitionDirectionalFeesSaturatesWithoutNetting(t *testing.T) {
	ask, bid, applied, err := TransitionDirectionalFees(17, MaxUint24-5, 10, DirectionXToY)
	require.NoError(t, err)
	assert.Equal(t, uint32(17), ask)
	assert.Equal(t, MaxUint24, bid)
	assert.Equal(t, uint32(5), applied)

	ask, bid, applied, err = TransitionDirectionalFees(17, 23, 7, DirectionYToX)
	require.NoError(t, err)
	assert.Equal(t, uint32(24), ask)
	assert.Equal(t, uint32(23), bid)
	assert.Equal(t, uint32(7), applied)
}

func TestQuoteChargesImmediatePunishmentWithoutMutatingState(t *testing.T) {
	params := testPool()
	params.ReserveX.SetUint64(1_000_000)
	params.ReserveY.SetUint64(1_000_000)
	params.FeeBidX24 = Q24Scale / 100
	params.MaxPunishmentX24 = Q24Scale / 10
	before := clonePool(params)

	quote, err := QuoteXToYChecked(params, uint256.NewInt(1_000_000))
	require.NoError(t, err)
	assert.Equal(t, uint256.NewInt(940_000), quote.AmountOut)
	assert.Equal(t, uint256.NewInt(60_000), quote.Fee, "stored fee and triggering punishment must both be charged")
	assert.Equal(t, uint32(Q24Scale/100+838_861), quote.EffectiveFeeX24)
	assert.Equal(t, params.SqrtPriceX96, quote.SqrtPriceNext)
	assertPoolEqual(t, before, params)

	multiplied, err := QuoteXToYWithMultiplierChecked(params, uint256.NewInt(1_000_000), uint256.NewInt(3))
	require.NoError(t, err)
	assert.Equal(t, uint256.NewInt(820_000), multiplied.AmountOut)
	assert.Equal(t, uint256.NewInt(180_000), multiplied.Fee, "multiplier must apply to the effective fee")
	assertPoolEqual(t, before, params)
}

func TestSimulateStandardTokenSwapTransitionsFeesAndReserves(t *testing.T) {
	params := testPool()
	params.ReserveX.SetUint64(1_000_000)
	params.ReserveY.SetUint64(1_000_000)
	params.MaxPunishmentX24 = Q24Scale / 10

	first, err := SimulateStandardTokenSwap(params, uint256.NewInt(1_000_000), DirectionXToY)
	require.NoError(t, err)
	assert.Equal(t, uint256.NewInt(950_000), first.AmountOut)
	assert.Equal(t, uint256.NewInt(50_000), first.Fee)
	assert.Equal(t, uint32(838_861), first.EffectiveFeeX24)
	assert.Equal(t, params.SqrtPriceX96, first.SqrtPriceNext)
	assert.Equal(t, uint32(838_861), first.DesiredPunishmentX24)
	assert.Equal(t, uint32(838_861), first.AppliedPunishmentX24)
	assert.Equal(t, uint32(0), params.FeeAskX24)
	assert.Equal(t, uint32(838_861), params.FeeBidX24)
	assert.Equal(t, uint256.NewInt(2_000_000), params.ReserveX)
	assert.True(t, params.ReserveY.IsZero())

	second, err := SimulateStandardTokenSwap(params, uint256.NewInt(1_000_000), DirectionYToX)
	require.NoError(t, err)
	assert.Equal(t, uint256.NewInt(950_000), second.AmountOut)
	assert.Equal(t, uint256.NewInt(50_000), second.Fee)
	assert.Equal(t, uint32(838_861), params.FeeAskX24)
	assert.Equal(t, uint32(838_861), params.FeeBidX24, "opposite direction does not net bid punishment")
	assert.Equal(t, uint256.NewInt(1_000_000), params.ReserveX)
	assert.Equal(t, uint256.NewInt(1_000_000), params.ReserveY)
}

func TestFullImmediatePunishmentSplitRegressionMatchesSolidity(t *testing.T) {
	base := testPool()
	base.ReserveX = u("1000000000000000000000000")
	base.ReserveY = u("1000000000000000000000000")
	base.FeeAskX24 = 0
	base.FeeBidX24 = 0
	base.MaxPunishmentX24 = MaxUint24

	singleState := clonePool(base)
	single, err := SimulateStandardTokenSwap(
		singleState,
		u("1000000000000000000000000"),
		DirectionXToY,
	)
	require.NoError(t, err)
	assert.Equal(t, "500000000000000000000000", single.AmountOut.Dec())
	assert.Equal(t, uint32(8_388_608), singleState.FeeBidX24)

	splitState := clonePool(base)
	splitTotalOut := new(uint256.Int)
	chunk := u("100000000000000000000000")
	for range 10 {
		step, stepErr := SimulateStandardTokenSwap(splitState, chunk, DirectionXToY)
		require.NoError(t, stepErr)
		splitTotalOut.Add(splitTotalOut, step.AmountOut)
	}

	assert.Equal(t, "724999934434890747070315", splitTotalOut.Dec())
	assert.Equal(t, uint32(8_388_610), splitState.FeeBidX24)
	assert.True(t, splitTotalOut.Gt(single.AmountOut))
}

func TestSimulationRemovesGrossOutputFromActiveReserve(t *testing.T) {
	params := testPool()
	params.ReserveX.SetUint64(1000)
	params.ReserveY.SetUint64(1000)
	params.FeeBidX24 = Q24Scale / 10

	result, err := SimulateStandardTokenSwap(params, uint256.NewInt(100), DirectionXToY)
	require.NoError(t, err)
	assert.Equal(t, uint256.NewInt(91), result.AmountOut)
	assert.Equal(t, uint256.NewInt(9), result.Fee)
	assert.Equal(t, uint256.NewInt(1100), params.ReserveX)
	assert.Equal(t, uint256.NewInt(900), params.ReserveY, "active reserve excludes the retained fee bucket")
}

func TestSimulationReturnsStructuredRollbackState(t *testing.T) {
	t.Run("input reserve exceeds uint112", func(t *testing.T) {
		params := testPool()
		params.ReserveX.Sub(new(uint256.Int).Lsh(uint256.NewInt(1), 112), uint256.NewInt(1))
		params.MaxPunishmentX24 = 100
		before := clonePool(params)
		result, err := SimulateStandardTokenSwap(params, uint256.NewInt(1), DirectionXToY)
		require.NoError(t, err)
		require.NotNil(t, result)
		assert.Equal(t, SimulationStatusReserveTransitionOverflow, result.Status)
		assert.False(t, result.Executable)
		assert.Equal(t, uint256.NewInt(1), result.AmountOut)
		assert.Equal(t, uint32(1), result.DesiredPunishmentX24)
		assert.Equal(t, uint32(1), result.EffectiveFeeX24)
		assert.Zero(t, result.AppliedPunishmentX24)
		assertPoolEqual(t, before, params)
	})

	t.Run("full fee makes swap impossible", func(t *testing.T) {
		params := testPool()
		params.FeeBidX24 = MaxUint24
		params.MaxPunishmentX24 = 100
		before := clonePool(params)
		result, err := SimulateStandardTokenSwap(params, uint256.NewInt(1), DirectionXToY)
		require.NoError(t, err)
		require.NotNil(t, result)
		assert.Equal(t, SimulationStatusSwapImpossible, result.Status)
		assert.False(t, result.Executable)
		assert.True(t, result.AmountOut.IsZero())
		assert.Equal(t, uint256.NewInt(1), result.Fee)
		assert.Equal(t, uint32(1), result.DesiredPunishmentX24)
		assert.Equal(t, MaxUint24, result.EffectiveFeeX24)
		assert.Zero(t, result.AppliedPunishmentX24)
		assertPoolEqual(t, before, params)
	})

	t.Run("triggering punishment saturates fee", func(t *testing.T) {
		params := testPool()
		params.ReserveX.SetUint64(100)
		params.ReserveY.SetUint64(100)
		params.FeeBidX24 = MaxUint24 - 5
		params.MaxPunishmentX24 = 1000
		before := clonePool(params)

		quote, err := QuoteXToYChecked(params, uint256.NewInt(1))
		require.NoError(t, err)
		assert.True(t, quote.AmountOut.IsZero())
		assert.Equal(t, uint256.NewInt(1), quote.Fee)
		assertPoolEqual(t, before, params)

		result, err := SimulateStandardTokenSwap(params, uint256.NewInt(1), DirectionXToY)
		require.NoError(t, err)
		assert.Equal(t, SimulationStatusSwapImpossible, result.Status)
		assert.Equal(t, quote.AmountOut, result.AmountOut)
		assert.Equal(t, quote.Fee, result.Fee)
		assert.Equal(t, quote.EffectiveFeeX24, result.EffectiveFeeX24)
		assert.Equal(t, uint32(5), result.DesiredPunishmentX24)
		assert.Zero(t, result.AppliedPunishmentX24)
		assertPoolEqual(t, before, params)
	})

	t.Run("mulDiv quotient overflow", func(t *testing.T) {
		params := testPool()
		params.SqrtPriceX96.Sub(new(uint256.Int).Lsh(uint256.NewInt(1), 160), uint256.NewInt(1))
		before := clonePool(params)
		max := new(uint256.Int).Not(new(uint256.Int))
		_, err := SimulateStandardTokenSwap(params, max, DirectionXToY)
		assert.True(t, errors.Is(err, ErrMathOverflow), err)
		assertPoolEqual(t, before, params)
	})
}

func TestSimulationMarkRolledBackRestoresAppliedState(t *testing.T) {
	params := testPool()
	params.MaxPunishmentX24 = 10_000
	before := clonePool(params)

	result, err := SimulateStandardTokenSwap(params, uint256.NewInt(100_000), DirectionXToY)
	require.NoError(t, err)
	require.Equal(t, SimulationStatusApplied, result.Status)
	require.True(t, result.Executable)
	require.Positive(t, result.DesiredPunishmentX24)
	require.Positive(t, result.AppliedPunishmentX24)
	amountOut := new(uint256.Int).Set(result.AmountOut)
	fee := new(uint256.Int).Set(result.Fee)
	effectiveFee := result.EffectiveFeeX24
	desired := result.DesiredPunishmentX24

	require.NoError(t, result.MarkRolledBack(RollbackReasonLaterRevert))
	assert.Equal(t, SimulationStatusLaterRevert, result.Status)
	assert.False(t, result.Executable)
	assert.Zero(t, result.AppliedPunishmentX24)
	assert.Equal(t, amountOut, result.AmountOut, "counterfactual quote remains observable")
	assert.Equal(t, fee, result.Fee)
	assert.Equal(t, effectiveFee, result.EffectiveFeeX24)
	assert.Equal(t, desired, result.DesiredPunishmentX24)
	assertPoolEqual(t, before, params)

	err = result.MarkRolledBack(RollbackReasonLaterRevert)
	assert.ErrorIs(t, err, ErrInvalidArgument, "a rollback result cannot restore state twice")

	newer := testPool()
	newer.MaxPunishmentX24 = 10_000
	stale, err := SimulateStandardTokenSwap(newer, uint256.NewInt(100_000), DirectionXToY)
	require.NoError(t, err)
	newer.FeeAskX24 = 1
	newerState := clonePool(newer)
	err = stale.MarkRolledBack(RollbackReasonLaterRevert)
	assert.ErrorIs(t, err, ErrInvalidArgument, "stale result must not overwrite newer state")
	assertPoolEqual(t, newerState, newer)
}

func TestCheckedIntoMathErrorsClearPreviousResults(t *testing.T) {
	params := testPool()
	params.SqrtPriceX96.Sub(new(uint256.Int).Lsh(uint256.NewInt(1), 160), uint256.NewInt(1))
	max := new(uint256.Int).Not(new(uint256.Int))
	before := clonePool(params)

	quote := newQuoteResult()
	quote.AmountOut.SetUint64(11)
	quote.SqrtPriceNext.SetUint64(12)
	quote.Fee.SetUint64(13)
	quote.EffectiveFeeX24 = 14
	err := QuoteXToYIntoChecked(quote, params, max)
	require.ErrorIs(t, err, ErrMathOverflow)
	assert.True(t, quote.AmountOut.IsZero())
	assert.True(t, quote.SqrtPriceNext.IsZero())
	assert.True(t, quote.Fee.IsZero())
	assert.Zero(t, quote.EffectiveFeeX24)
	quote.AmountOut.SetUint64(31)
	quote.SqrtPriceNext.SetUint64(32)
	quote.Fee.SetUint64(33)
	quote.EffectiveFeeX24 = 34
	err = QuoteXToYIntoChecked(quote, params, nil)
	require.ErrorIs(t, err, ErrInvalidArgument)
	assert.True(t, quote.AmountOut.IsZero())
	assert.True(t, quote.SqrtPriceNext.IsZero())
	assert.True(t, quote.Fee.IsZero())
	assert.Zero(t, quote.EffectiveFeeX24)

	simulation := newSwapSimulationResult()
	simulation.AmountOut.SetUint64(21)
	simulation.SqrtPriceNext.SetUint64(22)
	simulation.Fee.SetUint64(23)
	simulation.EffectiveFeeX24 = 24
	simulation.DesiredPunishmentX24 = 25
	simulation.AppliedPunishmentX24 = 26
	simulation.Executable = true
	simulation.Status = SimulationStatusApplied
	err = SimulateStandardTokenSwapInto(simulation, params, max, one, DirectionXToY)
	require.ErrorIs(t, err, ErrMathOverflow)
	assert.True(t, simulation.AmountOut.IsZero())
	assert.True(t, simulation.SqrtPriceNext.IsZero())
	assert.True(t, simulation.Fee.IsZero())
	assert.Zero(t, simulation.EffectiveFeeX24)
	assert.Zero(t, simulation.DesiredPunishmentX24)
	assert.Zero(t, simulation.AppliedPunishmentX24)
	assert.False(t, simulation.Executable)
	assert.Equal(t, SimulationStatusUnknown, simulation.Status)
	assertPoolEqual(t, before, params)

	simulation.AmountOut.SetUint64(41)
	simulation.SqrtPriceNext.SetUint64(42)
	simulation.Fee.SetUint64(43)
	simulation.EffectiveFeeX24 = 44
	simulation.DesiredPunishmentX24 = 45
	simulation.AppliedPunishmentX24 = 46
	simulation.Executable = true
	simulation.Status = SimulationStatusApplied
	err = SimulateStandardTokenSwapInto(simulation, params, uint256.NewInt(1), one, SwapDirection(2))
	require.ErrorIs(t, err, ErrInvalidArgument)
	assert.True(t, simulation.AmountOut.IsZero())
	assert.True(t, simulation.SqrtPriceNext.IsZero())
	assert.True(t, simulation.Fee.IsZero())
	assert.Zero(t, simulation.EffectiveFeeX24)
	assert.Zero(t, simulation.DesiredPunishmentX24)
	assert.Zero(t, simulation.AppliedPunishmentX24)
	assert.False(t, simulation.Executable)
	assert.Equal(t, SimulationStatusUnknown, simulation.Status)
	assertPoolEqual(t, before, params)
}

func TestSimulationRejectsAliasesBeforeMutation(t *testing.T) {
	t.Run("mutable state fields", func(t *testing.T) {
		params := testPool()
		params.ReserveY = params.ReserveX
		reserveBefore := new(uint256.Int).Set(params.ReserveX)
		_, err := SimulateStandardTokenSwap(params, uint256.NewInt(1), DirectionXToY)
		assert.ErrorIs(t, err, ErrInvalidArgument)
		assert.Equal(t, reserveBefore, params.ReserveX)
		assert.Same(t, params.ReserveX, params.ReserveY)
	})

	t.Run("result and amountIn", func(t *testing.T) {
		params := testPool()
		before := clonePool(params)
		amountIn := uint256.NewInt(10)
		out := newSwapSimulationResult()
		out.AmountOut = amountIn
		err := SimulateStandardTokenSwapInto(out, params, amountIn, one, DirectionXToY)
		assert.ErrorIs(t, err, ErrInvalidArgument)
		assert.Equal(t, uint256.NewInt(10), amountIn)
		assertPoolEqual(t, before, params)
	})

	t.Run("quote result and amountIn", func(t *testing.T) {
		params := testPool()
		before := clonePool(params)
		amountIn := uint256.NewInt(10)
		out := newQuoteResult()
		out.AmountOut = amountIn
		err := QuoteXToYIntoChecked(out, params, amountIn)
		assert.ErrorIs(t, err, ErrInvalidArgument)
		assert.Equal(t, uint256.NewInt(10), amountIn)
		assertPoolEqual(t, before, params)
	})
}

func TestApplyStateUpdateResetsFeesAndPreservesConfigAndReserves(t *testing.T) {
	params := testPool()
	params.FeeAskX24 = 100
	params.FeeBidX24 = 200
	params.MaxPunishmentX24 = 300
	reserveX := new(uint256.Int).Set(params.ReserveX)
	reserveY := new(uint256.Int).Set(params.ReserveY)
	newAnchor := new(uint256.Int).Mul(q96, uint256.NewInt(2))

	require.NoError(t, ApplyStateUpdate(params, newAnchor, 7, 11))
	assert.Equal(t, newAnchor, params.SqrtPriceX96)
	assert.Equal(t, uint32(7), params.FeeAskX24)
	assert.Equal(t, uint32(11), params.FeeBidX24)
	assert.Equal(t, uint32(300), params.MaxPunishmentX24)
	assert.Equal(t, reserveX, params.ReserveX)
	assert.Equal(t, reserveY, params.ReserveY)

	before := clonePool(params)
	err := ApplyStateUpdate(params, newAnchor, Q24Scale, 0)
	assert.ErrorIs(t, err, ErrValueOutOfRange)
	assertPoolEqual(t, before, params)

	params.ReserveY = params.ReserveX
	err = ApplyStateUpdate(params, newAnchor, 0, 0)
	assert.ErrorIs(t, err, ErrInvalidArgument)
}
