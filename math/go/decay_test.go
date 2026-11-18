package lunarbasepmm

import (
	"github.com/holiman/uint256"
	"github.com/stretchr/testify/require"
	"testing"
)

func TestCubicPriceAgeGoldenFeesAndFreshness(t *testing.T) {
	params := &PoolParams{
		SqrtPriceX96: new(uint256.Int).Set(q96),
		ReserveX:     uint256.NewInt(500_000), ReserveY: uint256.NewInt(100_000_000),
		FeeBidX24: 10_000, LatestUpdateBlock: 100, BlockDelay: 10,
		MaxDecayMultiplierX24: 3 * uint64(Q24Scale),
	}
	for _, fixture := range []struct{ age, fee uint64 }{
		{0, 10_000}, {1, 10_020}, {5, 12_500}, {9, 24_580}, {10, 30_000},
	} {
		params.CurrentBlock = 100 + fixture.age
		quote, err := QuoteXToYChecked(params, uint256.NewInt(uint64(Q24Scale)))
		require.NoError(t, err)
		require.Equal(t, fixture.fee, quote.Fee.Uint64())
		require.Equal(t, uint32(10_000), quote.EffectiveFeeX24)
	}
	params.CurrentBlock = 111
	quote, err := QuoteXToYChecked(params, uint256.NewInt(uint64(Q24Scale)))
	require.NoError(t, err)
	require.True(t, quote.AmountOut.IsZero())
}

func TestAgeFeeDoesNotPersistAndUpdateResetsAge(t *testing.T) {
	params := &PoolParams{
		SqrtPriceX96: new(uint256.Int).Set(q96),
		ReserveX:     uint256.NewInt(500_000), ReserveY: uint256.NewInt(500_000),
		FeeBidX24: 10_000, MaxPunishmentX24: 16_778,
		CurrentBlock: 105, LatestUpdateBlock: 100, BlockDelay: 10,
		MaxDecayMultiplierX24: 3 * uint64(Q24Scale),
	}
	result, err := SimulateStandardTokenSwap(params, uint256.NewInt(1_000), DirectionXToY)
	require.NoError(t, err)
	require.Equal(t, uint32(10_001), result.EffectiveFeeX24)
	require.Equal(t, uint32(10_001), params.FeeBidX24)
	require.Equal(t, uint64(100), params.LatestUpdateBlock)
	require.NoError(t, ApplyStateUpdate(params, q96, 10_000, 10_000))
	require.Equal(t, uint64(105), params.LatestUpdateBlock)
}

func TestOperatorUpdateTruncatesBlockToUint48(testingContext *testing.T) {
	params := testPool()
	params.CurrentBlock = (uint64(1) << 48) + 7
	require.NoError(testingContext, ApplyStateUpdate(params, q96, 1, 2))
	require.Equal(testingContext, uint64(7), params.LatestUpdateBlock)
}
