# Adaptive orderbook precision: reproducible sample

These are **synthetic, deterministic test states**, not live Pool telemetry,
market-wide guarantees, or a recommended production depth/risk configuration.

Run from the math repository:

```sh
cargo run -p lunarbase-pmm-math --release --example orderbook_precision
```

The example compares a baseline 20-rung geometric sampler,
a fitted one-level book, and the new adaptive constrained fitter. An independent
breadth-first replay checks the final output against all reachable fill states
and recomputes its precision. Machine-readable snapshots/results, excluding
machine-dependent timing, are in [orderbook-precision-samples.json](orderbook-precision-samples.json).

## State and domain

- Pair: WETH (18 decimals) / USDC (6 decimals), X=WETH and Y=USDC.
- Active reserves: 100 WETH and 250,000 USDC.
- Anchor: `floor(Q96 / 20000) = 3961408125713216879677197`, approximately 2,500 USDC/WETH.
- Initial ask/bid fees: `floor(Q24 / 10000) = 1677`, approximately 1 bp.
- Caller multiplier: 1. Valid accounting with empty fee buckets is assumed.
- Input lots: 1 WETH / 2,500 USDC. A fill is 1 or 2 lots; each directional
  ladder has 8 lots of total lifetime depth. Both directions can interleave.
- Snapshot block 100, latest update 100, block delay 20, execution horizon 105.
- Each scenario covers 225 distinct states, 699 transitions and 30 distinct
  `(direction, cursor, amountIn)` output constraints after worst-state reduction.
- Precision target: 1 bp; at most 20 levels per direction. Graph limit 100,000
  transitions; fitting/replay limit 5,000,000 work units.

For every direction/cursor/input, define `Qmin` as the minimum actual Pool output
over the reachable states. The reported loss is the maximum of
`10000 * (Qmin - exactLadderOutput) / Qmin`. Values below are conservatively
rounded **up** to six decimal places in bps. The public API reports whole-bps
ceilings, so `0.500310 bps` is exposed as `worst_underquote_bps = 1`.

## Results

| Max punishment | Baseline geometric, 20/20 levels | Fitted 1/1 level | Adaptive worst loss | Adaptive levels X/Y | 1 bp target |
| --- | ---: | ---: | ---: | ---: | --- |
| 0 | 0.041843 bps | 0.000003 bps | 0.000003 bps | 1 / 1 | Met |
| `floor(Q24/100)`, about 1% | 3.500614 bps | 3.501106 bps | 0.500310 bps | 4 / 4 | Met |
| `floor(Q24/10)`, about 10% | 35.089893 bps | 35.022649 bps | 5.018301 bps | 6 / 8 | Not met |

The moderate case already meets the target with `max_levels=4`. With the cap at
20, it uses 12,908 fitting/replay work units and still returns only four levels
per direction. The stress case uses 30,726 units and returns `target_met=false`;
the publisher example withholds its levels. This is an observed heuristic miss,
**not** a mathematical proof that all possible 20-level ladders fail.

No raw-ladder overquote was found on any allowed transition in these three
scenarios, including the baseline sampler. The new builder certifies this
finite domain rather than relying solely on a continuous concavity assumption.

The separate fresh-snapshot discount ceilings are 1, 4 and 36 bps respectively.
They include reserve/punishment evolution. In particular, reaching 0.500310 bps
against the worst-state reference does **not** mean every later fill is within
that distance of the initial best quote.

## Why exact validation is still needed

The geometric baseline uses cumulative samples, chord prices rounded down and
a power-of-two size grid. The adaptive builder uses the same cumulative
`OrderBookLevel { size, price }` representation and geometric split candidates,
but fits prices against exact cursor-aware
constraints instead of treating continuous concavity as a proof for integer
Pool execution.

A separate existing raw-unit counterexample has anchor `1.5 * Q96`, zero fees
and zero punishment: `quote(1)=1`, `quote(2)=4`. Sampling the second point produces
a size-2, price-2 rung that promises 2 for input 1, while the Pool returns 1.
This demonstrates **one raw output unit** of overquote in that edge case; it is
not a bound on all errors or a statement about ordinary trade-sized inputs.

Tests also cover a two-tranche sweep whose exact output is 2, whereas an
output-to-average-price-to-output round trip produces 1. Settlement adapters
must therefore preserve the executing system's exact promised output as the
Pool's `amountOutMinimum` instead of reconstructing it from an average price.

## Scope

- Quotes come from one coherent cached snapshot. Reserve reconciliation,
  fee-accounting preflight, standard-token behavior, enforced lot/min/max input,
  matched directional depth, and signature-generation lifecycle remain required.
- Direct Pool activity, operator/config changes and other executable quote
  generations remain outside the modeled domain. The on-chain output floor
  still protects settlement when those assumptions change.
- Certification concerns **raw levels**. Packing boundaries/prices or merging
  levels changes integer tranche rounding and needs a new exact replay.
  A target system's integration tests do not establish a general guarantee for
  arbitrary encoding transformations or different matching semantics.
- Protocol-specific adapters must separately test cross-rung fills, mixed
  directions, lifetime cursors, exact minimum-output settlement and cache
  consistency against the target execution system and Pool implementation.
