package lunarbasepmm

import (
	"math"
	"testing"
)

func TestPriceToSqrtPriceX96Unit(t *testing.T) {
	p := PriceToSqrtPriceX96(1.0)
	if p.Cmp(q96) != 0 {
		t.Fatalf("price=1.0 expected %s, got %s", q96, p)
	}
	back := SqrtPriceX96ToPrice(p)
	if math.Abs(back-1.0) > 1e-15 {
		t.Fatalf("round-trip 1.0: got %v", back)
	}
}

func TestPriceX96RoundTripAssorted(t *testing.T) {
	for _, price := range []float64{0.25, 1.5, 2500.0, 1e-9, 1e9} {
		p := PriceToSqrtPriceX96(price)
		back := SqrtPriceX96ToPrice(p)
		relErr := math.Abs(back-price) / price
		if relErr > 1e-14 {
			t.Errorf("price=%v back=%v rel_err=%v", price, back, relErr)
		}
	}
}

func TestPriceZeroMapsToZero(t *testing.T) {
	if !PriceToSqrtPriceX96(0).IsZero() {
		t.Errorf("PriceToSqrtPriceX96(0) != 0")
	}
	if v := SqrtPriceX96ToPrice(nil); v != 0 {
		t.Errorf("SqrtPriceX96ToPrice(nil) = %v", v)
	}
	if v := SqrtPriceX96ToPrice(PriceToSqrtPriceX96(0)); v != 0 {
		t.Errorf("SqrtPriceX96ToPrice(0) = %v", v)
	}
}

func TestPriceNaNPanics(t *testing.T) {
	defer func() {
		if r := recover(); r == nil {
			t.Errorf("expected panic on NaN")
		}
	}()
	_ = PriceToSqrtPriceX96(math.NaN())
}

func TestPriceNegativePanics(t *testing.T) {
	defer func() {
		if r := recover(); r == nil {
			t.Errorf("expected panic on negative")
		}
	}()
	_ = PriceToSqrtPriceX96(-1.0)
}
