package lunarbasepmm

import (
	"testing"

	"github.com/holiman/uint256"
)

const q48u = uint64(1) << 48

func symmetricPool() *PoolParams {
	p := uint256.NewInt(q48u)
	return &PoolParams{
		SqrtPriceX48:       p,
		AnchorSqrtPriceX48: p,
		FeeAskX24:          (1 << 24) / 1_000, // 0.10%
		FeeBidX24:          (1 << 24) / 1_000, // 0.10%
		ReserveX:           uint256.NewInt(1_000_000_000_000_000_000),
		ReserveY:           uint256.NewInt(1_000_000_000_000_000_000),
		ConcentrationKQ12:  5_000,
	}
}

func asymmetricPool() *PoolParams {
	p := uint256.NewInt((q48u * 3) / 2)
	return &PoolParams{
		SqrtPriceX48:       p,
		AnchorSqrtPriceX48: p,
		FeeAskX24:          (1 << 24) / 100, // 1.00%
		FeeBidX24:          (1 << 24) / 333, // ~0.30%
		ReserveX:           uint256.NewInt(750_000_000_000_000_000),
		ReserveY:           uint256.NewInt(1_500_000_000_000_000_000),
		ConcentrationKQ12:  8_000,
	}
}

type quoteFn func(params *PoolParams, amount *uint256.Int) *QuoteResult
type quoteFnInto func(out *QuoteResult, params *PoolParams, amount *uint256.Int) *QuoteResult

func runBench(b *testing.B, fn quoteFn, params *PoolParams, amount *uint256.Int) {
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_ = fn(params, amount)
	}
}

// runBenchInto exercises the *Into API with a pre-allocated destination
// reused across iterations — the hot path stays at 0 allocs/op.
func runBenchInto(b *testing.B, fn quoteFnInto, params *PoolParams, amount *uint256.Int) {
	out := &QuoteResult{
		AmountOut:     new(uint256.Int),
		SqrtPriceNext: new(uint256.Int),
		Fee:           new(uint256.Int),
	}
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_ = fn(out, params, amount)
	}
}

func BenchmarkQuoteXToY_SymmetricMid(b *testing.B) {
	runBench(b, QuoteXToY, symmetricPool(), uint256.NewInt(10_000_000_000_000_000))
}
func BenchmarkQuoteXToY_NearBound(b *testing.B) {
	runBench(b, QuoteXToY, symmetricPool(), uint256.NewInt(900_000_000_000_000_000))
}
func BenchmarkQuoteXToY_TinyAmount(b *testing.B) {
	runBench(b, QuoteXToY, symmetricPool(), uint256.NewInt(1))
}
func BenchmarkQuoteXToY_RejectedTooLarge(b *testing.B) {
	runBench(b, QuoteXToY, symmetricPool(), uint256.NewInt(10_000_000_000_000_000_000))
}
func BenchmarkQuoteXToY_AsymmetricPool(b *testing.B) {
	runBench(b, QuoteXToY, asymmetricPool(), uint256.NewInt(10_000_000_000_000_000))
}

func BenchmarkQuoteYToX_SymmetricMid(b *testing.B) {
	runBench(b, QuoteYToX, symmetricPool(), uint256.NewInt(10_000_000_000_000_000))
}
func BenchmarkQuoteYToX_NearBound(b *testing.B) {
	runBench(b, QuoteYToX, symmetricPool(), uint256.NewInt(900_000_000_000_000_000))
}
func BenchmarkQuoteYToX_TinyAmount(b *testing.B) {
	runBench(b, QuoteYToX, symmetricPool(), uint256.NewInt(1))
}
func BenchmarkQuoteYToX_RejectedTooLarge(b *testing.B) {
	runBench(b, QuoteYToX, symmetricPool(), uint256.NewInt(10_000_000_000_000_000_000))
}
func BenchmarkQuoteYToX_AsymmetricPool(b *testing.B) {
	runBench(b, QuoteYToX, asymmetricPool(), uint256.NewInt(10_000_000_000_000_000))
}

// --- *Into variants: 0 allocs/op on the hot path ---

func BenchmarkQuoteXToYInto_SymmetricMid(b *testing.B) {
	runBenchInto(b, QuoteXToYInto, symmetricPool(), uint256.NewInt(10_000_000_000_000_000))
}
func BenchmarkQuoteXToYInto_NearBound(b *testing.B) {
	runBenchInto(b, QuoteXToYInto, symmetricPool(), uint256.NewInt(900_000_000_000_000_000))
}
func BenchmarkQuoteXToYInto_TinyAmount(b *testing.B) {
	runBenchInto(b, QuoteXToYInto, symmetricPool(), uint256.NewInt(1))
}
func BenchmarkQuoteXToYInto_RejectedTooLarge(b *testing.B) {
	runBenchInto(b, QuoteXToYInto, symmetricPool(), uint256.NewInt(10_000_000_000_000_000_000))
}
func BenchmarkQuoteXToYInto_AsymmetricPool(b *testing.B) {
	runBenchInto(b, QuoteXToYInto, asymmetricPool(), uint256.NewInt(10_000_000_000_000_000))
}
func BenchmarkQuoteYToXInto_SymmetricMid(b *testing.B) {
	runBenchInto(b, QuoteYToXInto, symmetricPool(), uint256.NewInt(10_000_000_000_000_000))
}
func BenchmarkQuoteYToXInto_NearBound(b *testing.B) {
	runBenchInto(b, QuoteYToXInto, symmetricPool(), uint256.NewInt(900_000_000_000_000_000))
}
func BenchmarkQuoteYToXInto_TinyAmount(b *testing.B) {
	runBenchInto(b, QuoteYToXInto, symmetricPool(), uint256.NewInt(1))
}
func BenchmarkQuoteYToXInto_RejectedTooLarge(b *testing.B) {
	runBenchInto(b, QuoteYToXInto, symmetricPool(), uint256.NewInt(10_000_000_000_000_000_000))
}
func BenchmarkQuoteYToXInto_AsymmetricPool(b *testing.B) {
	runBenchInto(b, QuoteYToXInto, asymmetricPool(), uint256.NewInt(10_000_000_000_000_000))
}
