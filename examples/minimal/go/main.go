// Minimal example: quote a swap in both directions and print the results.
//
// Run from the repo root:  go run ./examples/go
package main

import (
	"fmt"

	"github.com/holiman/uint256"

	pmm "github.com/Lunarbase-Lab/lunarbase-pmm-math/math/go"
)

func main() {
	p := new(uint256.Int).Lsh(uint256.NewInt(1), 96)
	params := &pmm.PoolParams{
		// Q64.96 = 2^96 represents price = 1.0.
		SqrtPriceX96:     p,
		FeeAskX24:        (1 << 24) / 1_000, // 0.10%
		FeeBidX24:        (1 << 24) / 1_000, // 0.10%
		ReserveX:         uint256.NewInt(1_000_000_000),
		ReserveY:         uint256.NewInt(1_000_000_000),
		MaxPunishmentX24: pmm.Q24Scale / 10, // max 10% increment at full inventory
	}

	dx := uint256.NewInt(10_000)
	r := pmm.QuoteXToY(params, dx)
	fmt.Printf("X->Y  in=%s  out=%s  fee=%s  effectiveFee=%d  pNext=%s\n",
		dx.Dec(), r.AmountOut.Dec(), r.Fee.Dec(), r.EffectiveFeeX24, r.SqrtPriceNext.Dec())

	after := &pmm.PoolParams{
		SqrtPriceX96:     new(uint256.Int).Set(params.SqrtPriceX96),
		FeeAskX24:        params.FeeAskX24,
		FeeBidX24:        params.FeeBidX24,
		ReserveX:         new(uint256.Int).Set(params.ReserveX),
		ReserveY:         new(uint256.Int).Set(params.ReserveY),
		MaxPunishmentX24: params.MaxPunishmentX24,
	}
	simulation, err := pmm.SimulateStandardTokenSwap(after, dx, pmm.DirectionXToY)
	if err != nil {
		panic(err)
	}
	fmt.Printf("      desiredPunishment=%d appliedPunishment=%d nextBidFee=%d\n",
		simulation.DesiredPunishmentX24, simulation.AppliedPunishmentX24, after.FeeBidX24)

	dy := uint256.NewInt(10_000)
	r = pmm.QuoteYToX(params, dy)
	fmt.Printf("Y->X  in=%s  out=%s  fee=%s  effectiveFee=%d  pNext=%s\n",
		dy.Dec(), r.AmountOut.Dec(), r.Fee.Dec(), r.EffectiveFeeX24, r.SqrtPriceNext.Dec())
}
