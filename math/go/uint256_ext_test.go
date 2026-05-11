package lunarbasepmm

import (
	"math"
	"testing"

	"github.com/holiman/uint256"
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

func TestPriceX48RoundTripUnit(t *testing.T) {
	p := PriceToSqrtPriceX48(1.0)
	if p.Uint64() != (1 << 48) {
		t.Fatalf("price=1.0 expected 2^48, got %d", p.Uint64())
	}
	back := SqrtPriceX48ToPrice(p)
	if math.Abs(back-1.0) > 1e-15 {
		t.Fatalf("round-trip 1.0: got %v", back)
	}
}

func TestPriceX48RoundTripAssorted(t *testing.T) {
	// Q48 == ~14.5 decimal digits — tolerance scales with 2/(p*2^48).
	for _, price := range []float64{0.25, 1.5, 2500.0, 1e-6, 1e6} {
		p := PriceToSqrtPriceX48(price)
		back := SqrtPriceX48ToPrice(p)
		relErr := math.Abs(back-price) / price
		if relErr > 1e-10 {
			t.Errorf("price=%v back=%v rel_err=%v", price, back, relErr)
		}
	}
}

func TestPriceZeroMapsToZero(t *testing.T) {
	if !PriceToSqrtPriceX96(0).IsZero() {
		t.Errorf("PriceToSqrtPriceX96(0) != 0")
	}
	if !PriceToSqrtPriceX48(0).IsZero() {
		t.Errorf("PriceToSqrtPriceX48(0) != 0")
	}
	if v := SqrtPriceX96ToPrice(new(uint256.Int)); v != 0 {
		t.Errorf("SqrtPriceX96ToPrice(0) = %v", v)
	}
	if v := SqrtPriceX48ToPrice(new(uint256.Int)); v != 0 {
		t.Errorf("SqrtPriceX48ToPrice(0) = %v", v)
	}
}

func TestX48X96LiftsPreservePrice(t *testing.T) {
	price := 1234.5
	pX48 := PriceToSqrtPriceX48(price)
	pX96 := SqrtPriceX48ToX96(pX48)
	back := SqrtPriceX96ToPrice(pX96)
	relErr := math.Abs(back-price) / price
	if relErr > 1e-12 {
		t.Errorf("lift round-trip: back=%v rel_err=%v", back, relErr)
	}
}

func TestPriceX48OverflowSaturates(t *testing.T) {
	huge := math.Ldexp(1, 64) // sqrt * 2^48 = 2^80, top of uint80 range
	p := PriceToSqrtPriceX48(huge)
	expected := new(uint256.Int).Sub(new(uint256.Int).Lsh(one, 80), one)
	if p.Cmp(expected) != 0 {
		t.Errorf("overflow expected %s, got %s", expected, p)
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
	_ = PriceToSqrtPriceX48(-1.0)
}
