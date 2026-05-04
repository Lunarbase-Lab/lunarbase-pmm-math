package lunarbasepmm

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"testing"

	"github.com/stretchr/testify/require"
)

type vector struct {
	Name   string `json:"name"`
	Dir    string `json:"dir"`
	PX48   string `json:"pX48"`
	Fee    string `json:"fee"`
	ResX   string `json:"resX"`
	ResY   string `json:"resY"`
	K      uint32 `json:"k"`
	Dx     string `json:"dx,omitempty"`
	Dy     string `json:"dy,omitempty"`
	PNext  string `json:"pNext"`
	FeeAmt string `json:"feeAmt"`
}

func runVectorFile(t *testing.T, path string) {
	t.Helper()
	f, err := os.Open(path)
	require.NoError(t, err)
	defer func() { _ = f.Close() }()

	var (
		total, xToY, yToX int
		failures          []string
	)

	sc := bufio.NewScanner(f)
	sc.Buffer(make([]byte, 1024*1024), 1024*1024)

	for line := 1; sc.Scan(); line++ {
		raw := sc.Text()
		if raw == "" {
			continue
		}

		var v vector
		require.NoError(t, json.Unmarshal([]byte(raw), &v), "parse error line %d", line)
		total++

		p := u(v.PX48)
		params := &PoolParams{
			SqrtPriceX48:       p,
			AnchorSqrtPriceX48: p,
			FeeQ48:             u(v.Fee).Uint64(),
			ReserveX:           u(v.ResX),
			ReserveY:           u(v.ResY),
			ConcentrationK:     v.K,
		}

		if v.Dir == "xToY" {
			xToY++
			r := QuoteXToY(params, u(v.Dx))
			if r.AmountOut.Dec() != v.Dy ||
				r.SqrtPriceNext.Dec() != v.PNext ||
				r.Fee.Dec() != v.FeeAmt {
				failures = append(failures, fmt.Sprintf(
					"%s line %d: xToY MISMATCH\n  dy:    got %s expected %s\n  pNext: got %s expected %s\n  fee:   got %s expected %s",
					v.Name, line, r.AmountOut.Dec(), v.Dy,
					r.SqrtPriceNext.Dec(), v.PNext,
					r.Fee.Dec(), v.FeeAmt))
			}
		} else {
			yToX++
			r := QuoteYToX(params, u(v.Dy))
			if r.AmountOut.Dec() != v.Dx ||
				r.SqrtPriceNext.Dec() != v.PNext ||
				r.Fee.Dec() != v.FeeAmt {
				failures = append(failures, fmt.Sprintf(
					"%s line %d: yToX MISMATCH\n  dx:    got %s expected %s\n  pNext: got %s expected %s\n  fee:   got %s expected %s",
					v.Name, line, r.AmountOut.Dec(), v.Dx,
					r.SqrtPriceNext.Dec(), v.PNext,
					r.Fee.Dec(), v.FeeAmt))
			}
		}
	}
	require.NoError(t, sc.Err())

	t.Logf("%s: %d total (%d xToY, %d yToX)", path, total, xToY, yToX)

	if len(failures) > 0 {
		show := failures
		if len(show) > 20 {
			show = show[:20]
		}
		for i, f := range show {
			t.Logf("[%d] %s", i+1, f)
		}
		if len(failures) > 20 {
			t.Logf("... and %d more", len(failures)-20)
		}
		t.Fatalf("%d out of %d vectors failed in %s", len(failures), total, path)
	}
}

func TestDeterministicVectors(t *testing.T) {
	runVectorFile(t, "testdata/deterministic_vectors.jsonl")
}

func TestFuzzVectors(t *testing.T) {
	runVectorFile(t, "testdata/fuzz_vectors.jsonl")
}
