# @lunarbase-lab/pmm-math

Native Node.js binding for the current LunarBase linear quote and immediate
directional-punishment math. Its outputs are replayed against deterministic
and seeded-fuzz JSONL vectors produced by Solidity.

## Install

```bash
npm install @lunarbase-lab/pmm-math
```

Supported native packages are macOS arm64, Linux x64 glibc (GLIBC 2.17+),
Linux arm64 glibc, and Linux x64 musl/Alpine. The x64 GNU release artifact is
cross-linked against the declared 2.17 floor and its imported symbol versions
are checked before publication.

## Usage

```ts
import {
  buildOrderBook,
  geometricSizes,
  OrderBookStatus,
  priceToSqrtPriceX96,
  quoteXToY,
  simulateXToY,
  SwapSimulationStatus,
  type QuoteParams,
} from "@lunarbase-lab/pmm-math";

const params = {
  sqrtPriceX96: priceToSqrtPriceX96(1),
  feeAskX24: 0,
  feeBidX24: 50_331,
  reserveX: "1000000000000000000000",
  reserveY: "1000000000000000000000",
  maxPunishmentX24: 1_677_721, // about 10% maximum increment
  amountIn: "1000000000000000000",
  feeMultiplier: "1",
} satisfies QuoteParams;

const quote = quoteXToY(params);
console.log(quote.amountOut, quote.sqrtPriceNext, quote.fee, quote.effectiveFeeX24);

const simulation = simulateXToY(params);
console.log({
  executable: simulation.executable,
  status: simulation.status,
  standardTokenTransitionApplied:
    simulation.status === SwapSimulationStatus.Applied,
  desiredPunishmentX24: simulation.desiredPunishmentX24,
  appliedPunishmentX24: simulation.appliedPunishmentX24,
  feeBidX24After: simulation.feeBidX24After,
  reserveXAfter: simulation.reserveXAfter,
  reserveYAfter: simulation.reserveYAfter,
});

const book = buildOrderBook({
  sqrtPriceX96: params.sqrtPriceX96,
  feeAskX24: params.feeAskX24,
  feeBidX24: params.feeBidX24,
  reserveX: params.reserveX,
  reserveY: params.reserveY,
  maxPunishmentX24: params.maxPunishmentX24,
  feeMultiplier: params.feeMultiplier,
  snapshotBlock: "100",
  maxExecutionBlock: "101", // last block covered by this signed quote policy
  latestUpdateBlock: "99",
  blockDelay: "3",
  paused: false,
  xToYSizes: geometricSizes("10000000000000000000", 20),
  yToXSizes: geometricSizes("10000000000", 20),
});
if (book.status === OrderBookStatus.Active) {
  if (!book.requiresAmountOutMinimum) throw new Error("unsafe projection");
  if (book.xToY.levels.length > 0) console.log(book.xToY.levels);
  if (book.yToX.levels.length > 0) console.log(book.yToX.levels);
}
```

`quoteXToY` uses the bid side; `quoteYToX` uses the ask side. Each triggering
quote computes its punishment from the pre-swap reserves, saturating-adds it to
the stored directional fee, and immediately prices output with that
`effectiveFeeX24`. Successful settlement persists the same fee; any rollback
leaves stored state unchanged.
`sqrtPriceNext` is retained for ABI compatibility and always equals
`sqrtPriceX96`.

`simulateXToY` and `simulateYToX` are counterfactual pure-math models of the
quote, punishment, and reserve transition for standard tokens. An `Applied`
status means that this local transition fits the Solidity numeric domains; it
does **not** guarantee that a transaction will succeed. Fee-on-transfer or
rebasing behavior, token callbacks, and later transfer/accounting reverts are
not modeled. `LaterRevert` mirrors the lower-level Rust rollback vocabulary but
is not produced automatically by these JavaScript functions.

`quoteXToY` and `quoteYToX` are stateless. Reusing unchanged params for every
chunk does not model a sequential split; pass the `fee*X24After` and
`reserve*After` values from each applied simulation into the next call. This is
economically material because splitting can increase aggregate output under
the current mechanism.

### API

| Function | Purpose |
| --- | --- |
| `quoteXToY(params)`, `quoteYToX(params)` | Triggering quote with immediate effective fee |
| `simulateXToY(params)`, `simulateYToX(params)` | Standard-token counterfactual quote, punishment, and reserve transition |
| `buildOrderBook(params)` | Cached Pool snapshot to two protocol-neutral directional cumulative-size ladders |
| `buildValidatedOrderBook(snapshot, config)` | Bounded exhaustive lot-policy book, including mixed-direction sequences |
| `buildPreciseOrderBook(snapshot, config, precision)` | Fit up to 20 conservative levels; report observed underquote and target attainment |
| `validateFeeAccountingCapacity(snapshot, config, accounting)` | Throw on uncredited partner fees or insufficient uint112 bucket headroom |
| `ladderAmountOut(levels, amountIn, cursor?)` | Exact per-tranche floor sum; `null` beyond depth |
| `geometricSizes(cap, levels)` | Power-of-two cumulative grid (maximum 20 levels) |
| `priceToSqrtPriceX96(price)`, `sqrtPriceX96ToPrice(p)` | Decimal price and Q64.96 anchor helpers |
| snake_case price helpers | Compatibility aliases |

`buildOrderBook` is deterministic and cache-agnostic. It requires one coherent
snapshot containing reserves, anchor, fees, maximum punishment, pause and
freshness fields, an explicit maximum execution block for the signed lifetime,
plus the caller-specific multiplier of the future execution adapter. It returns empty
sides with `paused` or `stale` status rather than
publishing unexecutable liquidity. Addresses, nonce, expiry, signature, event
subscription, and transport remain the publisher's responsibility.
An `active` status only passes the snapshot gate; publish a direction only when
its `levels` array is non-empty.

The order-book projection is not tied to a particular exchange or execution
protocol. Each direction uses cumulative raw input sizes and output-per-input
prices scaled by `1e18`; fills sum `floor(trancheInput * price / 1e18)` across
the consumed tranches, starting at the lifetime input cursor. Integrations
must map these levels to their own order format and match these execution
semantics, or independently validate the transformed representation.

Prices are conservative at emitted cumulative prefixes. Because the exact Pool
quote has nested integer floors and stateful immediate punishment, the future
on-chain adapter must still pass the swept `amountOut` as
`amountOutMinimum`; use short expiries and a tested execution lot grid.
The result makes this contract machine-visible as
`requiresAmountOutMinimum: true`.

`buildOrderBook` results have `safety: "indicative"`. For modeled execution
coverage use `buildValidatedOrderBook(snapshot, { xToY, yToX, maxTransitions })`.
Each direction is optional and contains decimal-string `minInput`, `lotInput`,
`maxInput`, `totalInput`. Its one-level price covers every allowed reachable
partial/sequential fill, including direction interleavings, from the snapshot.
`maxTransitions` must be an integer in 1..=100000; exhaustion throws instead of
returning a partial guarantee. An active certified result has
`book.safety === OrderBookSafety.ExhaustiveLotPolicy`, plus source snapshot,
config and coverage counts. The adapter must enforce the same policy and guard
external state changes with exact `amountOutMinimum`.
Before signing, call `validateFeeAccountingCapacity` with the same snapshot,
config and cached `FeeAccountingState`. It checks that partner share
(`BPS=1_000_000`) has an operator when nonzero, and that global treasury/partner
and per-router cumulative fee buckets have conservative uint112 headroom.
The certificate is a price/reserve/punishment-model guarantee under these
conditions and standard token behavior; it cannot guarantee arbitrary EVM calls.
Reconcile stored reserves against raw Pool token balances minus pending escrow
and global fee buckets before claiming exact transitions. Unsynced donations
already present at the snapshot are not detectable from the math inputs alone.

For a multilevel book, use `buildPreciseOrderBook` with the same snapshot,
accounting preflight and on-chain-enforced policy:

```ts
import { buildPreciseOrderBook, validateFeeAccountingCapacity } from "@lunarbase-lab/pmm-math";

validateFeeAccountingCapacity(snapshot, config, cachedFeeAccounting);
const precise = buildPreciseOrderBook(snapshot, config, {
  maxLevels: 20,
  targetUnderquoteBps: 10, // requested maximum 0.10% fitting gap
  maxWork: 1_000_000,
});
console.log({
  targetMet: precise.targetMet,
  worstUnderquoteBps: precise.worstUnderquoteBps,
  worstFreshSnapshotDiscountBps: precise.worstFreshSnapshotDiscountBps,
  workUsed: precise.workUsed,
  constraintCount: precise.constraintCount,
});
const { book } = precise.validated;
// Require precise.targetMet before publishing at the requested tolerance,
// plus the same active-status, non-empty-side and signing-generation checks.
```

`targetUnderquoteBps` is measured against the **worst admissible Pool state**
for the same direction, lifetime cursor and fill size. It is not a guaranteed
discount to a fresh snapshot's quote. Stateful punishment and opposite-direction
fills can make that worst state less favorable; the separate
`worstFreshSnapshotDiscountBps` reports the resulting discount to the initial
snapshot's same-size quote. Both metrics round relative gaps up to integer bps.

`targetMet` reports what the fit achieved; requesting a tolerance does not
guarantee it is feasible with the permitted levels and fills. A result with
`targetMet: false` still retains its exhaustive model safety. Paused/stale
results never claim target attainment. `maxLevels` must be an integer in
1..=20, `targetUnderquoteBps` in 0..=10000 and `maxWork` in 1..=5000000.
Fitting has a separate work budget from `config.maxTransitions`; exhausting
either budget throws and returns no unchecked approximation. The earlier
indicative and flat validated APIs retain their behavior.
Fitting is a bounded heuristic: missing the target is not proof that no ladder
could meet it. Its scores compare dimensionless relative gaps, so different
token decimals do not bias candidate selection.

The guarantee covers the exact returned raw levels. Revalidate any executor
packing, quantization, boundary rounding or level merging against the same
cursor constraints. Even merging equal-price levels can increase delivered
output by removing a tranche floor.

All `uint112`, `uint160`, and `uint256` fields cross the JS/native boundary as
strict strings. Decimal input must be exactly `"0"` or an unsigned digit string
without leading zeros, is limited to 78 digits, and may not exceed
`uint256::MAX`. Hex input requires a `0x` prefix and is limited to 64 hex
digits. Q24 fields are numbers but must be finite integers in `[0, 0xffffff]`;
fractional, wrapped, NaN, and infinite inputs are rejected before quoting.

## Pure Rust

The same implementation is published as
[`lunarbase-pmm-math`](https://crates.io/crates/lunarbase-pmm-math).

## License

MIT OR Apache-2.0.
