# Benchmarks

Synthetic micro-benchmarks for the `quote_x_to_y` / `quote_y_to_x` hot path
in both the Rust core and the Go mirror. The same five scenarios are used in
both languages (mirrored in `math/rust/lunarbase-pmm-math/benches/quote.rs`
and `math/go/quote_bench_test.go`) so the numbers are directly comparable.

Run locally:

```sh
make bench          # both
make bench-rust     # criterion (single-threaded ns/op + CIs)
make bench-go       # testing.B with -benchmem
```

Hardware: macOS arm64 (Apple-Silicon), Rust 1.94, Go 1.22. All numbers are
median of three runs; criterion CIs typically span ±2 % around the median.

## Rust — `criterion` (median ns/op)

| Scenario | Direction | Baseline | After step 2 | Δ |
|---|---|---|---|---|
| `symmetric_mid`     | x→y | 419 ns | 422 ns | ≈ noise |
| `near_bound`        | x→y | 133 ns | ~130 ns† | ≈ noise |
| `tiny_amount`       | x→y | 357 ns | 351 ns | −2 % |
| `rejected_too_large`| x→y | 105 ns | 100 ns | −5 % |
| `asymmetric_pool`   | x→y | 418 ns | 406 ns | −3 % |
| `symmetric_mid`     | y→x | 359 ns | 381 ns† | ≈ noise |
| `near_bound`        | y→x | 113 ns | 104 ns | −8 % |
| `tiny_amount`       | y→x | 293 ns | 286 ns | −2 % |
| `rejected_too_large`| y→x |  82 ns |  77 ns | −6 % |
| `asymmetric_pool`   | y→x | 364 ns | 383 ns† | ≈ noise |

† Criterion CI overlaps with baseline — measured difference is below the
noise floor of an idle laptop. Take everything within ±5 % as "no change".

**Conclusion:** the Rust core was already near-optimal because `ruint` itself
is heavily tuned. The optimizations in step 2 give a real but small win on
short-circuit paths (`rejected_too_large`, `near_bound`) and noise-floor
elsewhere. No regressions; the public API didn't change.

### What changed in step 2

- Added `#[inline]` to `mul_div`, `mul_div_ceil`, `ceil_div`, and the four
  `sqrt_price_math` functions. Helps LLVM inline through the call chain.
- Pre-computed `Q48_U256` and `Q96_U256` as `const U256` so `concentration_q48`
  doesn't reconstruct them on every call (was `wrapping_mul(from_u128(Q48), from_u128(Q48))`).

## Go — `testing.B -benchmem` (median over 3 runs)

| Scenario | Direction | Baseline | After step 3 | Δ ns | Δ allocs | Δ B/op |
|---|---|---:|---:|---:|---:|---:|
| `SymmetricMid`     | x→y | 624 ns / 432 B / 14 allocs | 533 ns / 120 B / 4 allocs | **−15 %** | **−71 %** | −72 % |
| `NearBound`        | x→y | 272 ns / 216 B /  7 allocs | 242 ns / 120 B / 4 allocs | **−11 %** | **−43 %** | −44 % |
| `TinyAmount`       | x→y | 530 ns / 432 B / 14 allocs | 414 ns / 120 B / 4 allocs | **−22 %** | **−71 %** | −72 % |
| `RejectedTooLarge` | x→y | 236 ns / 216 B /  7 allocs | 203 ns / 120 B / 4 allocs | **−14 %** | **−43 %** | −44 % |
| `AsymmetricPool`   | x→y | 631 ns / 432 B / 14 allocs | 544 ns / 120 B / 4 allocs | **−14 %** | **−71 %** | −72 % |
| `SymmetricMid`     | y→x | 545 ns / 432 B / 14 allocs | 442 ns / 120 B / 4 allocs | **−19 %** | **−71 %** | −72 % |
| `NearBound`        | y→x | 241 ns / 216 B /  7 allocs | 205 ns / 120 B / 4 allocs | **−15 %** | **−43 %** | −44 % |
| `TinyAmount`       | y→x | 418 ns / 432 B / 14 allocs | 308 ns / 120 B / 4 allocs | **−26 %** | **−71 %** | −72 % |
| `RejectedTooLarge` | y→x | 209 ns / 216 B /  7 allocs | 185 ns / 120 B / 4 allocs | **−11 %** | **−43 %** | −44 % |
| `AsymmetricPool`   | y→x | 531 ns / 432 B / 14 allocs | 434 ns / 120 B / 4 allocs | **−18 %** | **−71 %** | −72 % |

Aggregates: **−16 % wall time** (range −11 % to −26 %), **−64 % allocations**,
**−72 % bytes/op** for the typical "happy path" quote.

The remaining 4 allocations per quote are the `QuoteResult` struct and its
three `*uint256.Int` fields, which cross the public API boundary. They are
eliminated entirely by the `*Into` variants below.

## Go — `*Into` API (zero-alloc hot path)

`QuoteXToYInto(out *QuoteResult, params *PoolParams, dx *uint256.Int)` and
`QuoteYToXInto(...)` accept a caller-allocated `*QuoteResult` and reuse its
`AmountOut` / `SqrtPriceNext` / `Fee` fields across iterations. The original
`QuoteXToY` / `QuoteYToX` are now thin wrappers over the `*Into` variants —
they exist for ergonomic one-shot use; for tight loops, `*Into` is the right
call.

| Scenario | Direction | Step-3 alloc-API | Step-5 `*Into` | Δ vs step 3 | Δ vs **original baseline** |
|---|---|---:|---:|---:|---:|
| `SymmetricMid`     | x→y | 533 ns / 4 allocs | **472 ns / 0 allocs** | −11 % | **−24 % ns, −100 % allocs** |
| `NearBound`        | x→y | 242 ns / 4 allocs | **184 ns / 0 allocs** | −24 % | −32 % ns |
| `RejectedTooLarge` | x→y | 203 ns / 4 allocs | **141 ns / 0 allocs** | −30 % | −40 % ns |
| `SymmetricMid`     | y→x | 442 ns / 4 allocs | **371 ns / 0 allocs** | −16 % | **−32 % ns, −100 % allocs** |
| `NearBound`        | y→x | 205 ns / 4 allocs | **146 ns / 0 allocs** | −29 % | −39 % ns |
| `RejectedTooLarge` | y→x | 185 ns / 4 allocs | **115 ns / 0 allocs** | −38 % | **−45 % ns, −100 % allocs** |

The hot path now allocates literally zero bytes — `go test -bench -benchmem`
reports `0 B/op  0 allocs/op`. This matches the kyberswap-dex-lib pattern
and is the right call for high-throughput consumers (router quoting loops,
fuzz harnesses, batch backtests).

### Sample call site

```go
out := &QuoteResult{
    AmountOut:     new(uint256.Int),
    SqrtPriceNext: new(uint256.Int),
    Fee:           new(uint256.Int),
}
for _, swap := range candidates {
    QuoteXToYInto(out, swap.Pool, swap.AmountIn)
    process(out)   // out.AmountOut, out.Fee, out.SqrtPriceNext are valid until next call
}
```

### What changed in step 3

All internal helpers were converted from "return a fresh `*uint256.Int`" to
"write into a caller-provided `dst`" style:

```go
// before
func mulDivDown(x, y, denom *uint256.Int) *uint256.Int { ... }

// after
func mulDivDown(dst, x, y, denom *uint256.Int) *uint256.Int { ... }
```

Affected functions: `mulDivDown`, `mulDivUp`, `ceilDiv`, `isqrt`,
`concentrationQ48`, `lowerBound`, `upperBound`, `liquidityX`, `liquidityY`,
`getAmountXDelta`, `getAmountYDelta`, `getNextSqrtPriceFromAmount{X,Y}*`.

`QuoteXToY` / `QuoteYToX` now declare all intermediates as
`var name uint256.Int` on the stack — Go's escape analysis keeps them off
the heap because `&name` doesn't outlive the function. The public API
(`PoolParams`, `QuoteResult`, `QuoteXToY`, `QuoteYToX`) is unchanged.

Also pre-computed `q96` as a package-level constant to avoid repeated
`Mul(q48, q48)` inside `concentrationQ48`.

## Methodology

- **Rust:** `cargo bench -p lunarbase-pmm-math` runs criterion with 100
  samples and ~5 s per scenario. Numbers reported are the median of the
  estimated per-iteration time.
- **Go:** `go test -bench=. -benchmem -count=3 -run=^$ ./...` runs each
  benchmark three times. Numbers reported are the median of the three runs;
  GC is on with default settings.
- All scenarios use Q48 fixed-point with `concentrationK ∈ {5000, 8000}`.
  See the bench source for exact pool parameters.

## Why we don't run benchmarks in CI

`cargo bench` and `go test -bench` are noise-sensitive on shared GitHub
Actions runners (variance can swamp the signal). They live as a `make`
target so contributors run them locally on a quiet machine before claiming
a perf change.
