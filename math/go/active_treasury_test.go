package lunarbasepmm

import (
	"github.com/holiman/uint256"
	"github.com/stretchr/testify/require"
	"testing"
)

func TestActiveTreasuryOnlyDebitsCreditedPartnerFees(test *testing.T) {
	for _, direction := range []SwapDirection{DirectionXToY, DirectionYToX} {
		for _, share := range []uint32{0, 1, 333333, 500000, 1000000} {
			for _, hasOperator := range []bool{false, true} {
				params := testPool()
				params.ReserveX.SetUint64(1000000)
				params.ReserveY.SetUint64(1000000)
				params.FeeAskX24 = Q24Scale / 10
				params.FeeBidX24 = Q24Scale / 10
				result, err := SimulateStandardTokenSwapWithPartnerFee(params, uint256.NewInt(100003), one, direction, share, hasOperator)
				require.NoError(test, err)
				partner := new(uint256.Int)
				if hasOperator {
					partner.Mul(result.Fee, uint256.NewInt(uint64(share))).Div(partner, uint256.NewInt(1000000))
				}
				expected := new(uint256.Int).Sub(uint256.NewInt(1000000), new(uint256.Int).Add(result.AmountOut, partner))
				reserve := params.ReserveY
				if direction == DirectionYToX {
					reserve = params.ReserveX
				}
				require.Equal(test, expected, reserve)
			}
		}
	}
}
