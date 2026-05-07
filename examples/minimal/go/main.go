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
	p := uint256.NewInt(1 << 48)
	params := &pmm.PoolParams{
		SqrtPriceX48:       p,
		AnchorSqrtPriceX48: p,
		FeeAskX24:          (1 << 24) / 1_000, // 0.10%
		FeeBidX24:          (1 << 24) / 1_000, // 0.10%
		ReserveX:           uint256.NewInt(1_000_000_000),
		ReserveY:           uint256.NewInt(1_000_000_000),
		ConcentrationKQ12:  5_000 << 12, // legacy K=5000 in Q20.12 encoding
	}

	dx := uint256.NewInt(10_000)
	r := pmm.QuoteXToY(params, dx)
	fmt.Printf("X->Y  in=%s  out=%s  fee=%s  pNext=%s\n",
		dx.Dec(), r.AmountOut.Dec(), r.Fee.Dec(), r.SqrtPriceNext.Dec())

	dy := uint256.NewInt(10_000)
	r = pmm.QuoteYToX(params, dy)
	fmt.Printf("Y->X  in=%s  out=%s  fee=%s  pNext=%s\n",
		dy.Dec(), r.AmountOut.Dec(), r.Fee.Dec(), r.SqrtPriceNext.Dec())
}
