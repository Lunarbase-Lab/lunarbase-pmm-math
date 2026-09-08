# Pool snapshot to a protocol-neutral order book

`lunarbase-pmm-math` exposes a pure `OrderBookState -> OrderBook` conversion
for LunarBase Pool quotes. It is independent of the order-book, routing or
matching system consuming the result. It does not open RPC/WebSocket
connections, mutate a cache, sign messages, or submit transactions.

The output contains two directional exact-input ladders:

- `xToY`: token X is `tokenIn`, token Y is `tokenOut`;
- `yToX`: token Y is `tokenIn`, token X is `tokenOut`.

Each level is `{ size, price }`, where `size` is cumulative raw `tokenIn`
volume and `price` is marginal raw `tokenOut / tokenIn`, scaled by `1e18`.
These are this library's representation and execution conventions, not an
assumption about every target system's wire format. The current implementation
accepts at most 20 levels per direction. An integration adapter is responsible
for mapping directions, cumulative versus per-level quantities, token units,
price scales, fill granularity and consumed-depth cursors to its target system.
Revalidate transformations that change the output promised for any allowed
fill, including packing, price rounding or merging levels. The mathematical
coverage is not automatically transferable to different matching semantics.

Market identifiers, addresses, nonces, expiry encoding, signatures and
publication transport belong to the integration layer. Settlement adapters
must enforce the configured fill policy and the target system's exact promised
output as the Pool's `amountOutMinimum`.

## Policy-validated TypeScript usage

Use `buildValidatedOrderBook` for a flat finite, enforced fill domain, or
`buildPreciseOrderBook` for multilevel fitting in the same domain. The sampled
`buildOrderBook` API remains available for indicative depth and carries
`safety: "indicative"` even when its snapshot is active.

```ts
import {
  buildValidatedOrderBook,
  validateFeeAccountingCapacity,
  OrderBookSafety,
  OrderBookStatus,
  type OrderBookSnapshot,
} from "@lunarbase-lab/pmm-math";

const snapshot: OrderBookSnapshot = {
  sqrtPriceX96: cached.anchorPrice,
  feeAskX24: cached.feeAskX24,
  feeBidX24: cached.feeBidX24,
  reserveX: cached.reserveX,
  reserveY: cached.reserveY,
  maxPunishmentX24: cached.maxPunishmentX24,
  feeMultiplier: cached.adapterFeeMultiplier,
  snapshotBlock: cached.blockNumber,
  // Must cover the complete signed TTL, not only the next block.
  maxExecutionBlock: quotePolicy.maxExecutionBlock,
  latestUpdateBlock: cached.latestUpdateBlock,
  blockDelay: cached.blockDelay,
  paused: cached.paused,
};

// Example WETH/USDC raw units: four lots of 0.001 WETH / 2 USDC.
// These bounds must match the adapter's enforced input policy and signed caps.
const config = {
  xToY: {
    minInput: "1000000000000000", lotInput: "1000000000000000",
    maxInput: "2000000000000000", totalInput: "4000000000000000",
  },
  yToX: {
    minInput: "2000000", lotInput: "2000000",
    maxInput: "4000000", totalInput: "8000000",
  },
  maxTransitions: 10000,
};
// All buckets/config belong to the same block snapshot and exact Pool caller.
// partnerFee uses Pool FeeManager.BPS = 1_000_000, not Q24.
validateFeeAccountingCapacity(snapshot, config, {
  partnerFee: cached.adapterPartnerFee,
  partnerOperatorPresent: cached.adapterPartnerOperator !== ZERO_ADDRESS,
  treasuryX: cached.treasuryFeesX, treasuryY: cached.treasuryFeesY,
  partnerX: cached.partnerFeesX, partnerY: cached.partnerFeesY,
  routerPartnerX: cached.adapterCumFeesX,
  routerPartnerY: cached.adapterCumFeesY,
});
const validated = buildValidatedOrderBook(snapshot, config);
const { book } = validated;
if (book.status !== OrderBookStatus.Active ||
    book.safety !== OrderBookSafety.ExhaustiveLotPolicy) {
  // Publish no liquidity for paused or stale state.
  return;
}

if (!book.requiresAmountOutMinimum) {
  throw new Error("unsupported unsafe projection contract");
}

if (book.xToY.levels.length > 0) {
  await publish("xToY", book.xToY.levels);
}
if (book.yToX.levels.length > 0) {
  await publish("yToX", book.yToX.levels);
}
```

`minInput`, `maxInput` and `totalInput` must be positive multiples of `lotInput`
and ordered `minInput <= maxInput <= totalInput`. A fill is permitted exactly
when `minInput <= amountIn <= maxInput`, `amountIn % lotInput == 0`, and its
direction's lifetime cursor plus input does not exceed `totalInput`. Omitting a
direction disables it. Caps use different raw token units on each side.

The certified quote/reserve/punishment model explores every reachable state/cursor pair and every
allowed outgoing fill, including all direction interleavings. It simulates the
actual Pool output and resulting reserves/punishment; it does not substitute the
smaller promised ladder output into the reserve transition. It emits one flat
level per enabled direction with price
`min(floor(actualOutput * 1e18 / amountIn))` over all edges in that direction.
For every allowed fill, `floor(amountIn * price / 1e18) <= actualOutput`,
independently of the lifetime cursor. The result returns its policy, source
snapshot and `checkedStates`/`checkedTransitions` for inspection.

The graph may grow exponentially. Start with 3–4 lots per direction, choose an
explicit work budget, and measure runtime. A hard ceiling of 100,000 transitions
bounds computation and state storage. Budget exhaustion, any reachable
unexecutable fill, or a price/minimum output rounding to zero fails closed with
an error. Prices are conservative and can be below a sampled curve's price.

Coverage applies only to sequences beginning at the supplied snapshot under the
exact same policy, fee multiplier, and paired directional caps. Unmodeled
direct swaps, operator updates, reserve changes, policy changes, concurrently
live quote generations, and cursor resets break those assumptions. Signed
generations need coordinated retirement; replacing one side or stopping a
publisher is not an on-chain cancellation. Always preserve `amountOutMinimum`
in the adapter to reject under-delivery after external state changes.

This is a price/reserve/punishment-model guarantee, not a promise that every EVM
call succeeds. Its reserve transitions assume all charged fees are credited:
either the adapter's `partnerFee` is zero, or its partner operator is nonzero.
A positive partner share with no operator causes Pool to skip partner credit,
so subtracting full gross output from active reserves would be incorrect.
Treasury, global partner, and per-router cumulative fee buckets are uint112 and
can overflow independently of otherwise valid active reserves.

Before signing, call `try_validate_fee_accounting_capacity` (Node:
`validateFeeAccountingCapacity`) using the same snapshot. It rejects partner
shares above `FeeManager.BPS = 1_000_000` and missing required operators. For an
enabled output token it conservatively bounds all future charged fees by that
token's initial active reserve plus its signed cumulative input cap, then
requires the relevant current fee buckets plus this bound to fit uint112.
For a disabled output direction the bound is zero; zero-share buckets do not
grow, but every supplied bucket is width-checked. The intentionally generous
bound may reject feasible books close to accounting limits.

Standard non-rebasing/non-fee-on-transfer ERC20 behavior, successful transfers,
required access permissions and unchanged implementations remain integration
preconditions. `SimulationStatus::Applied` describes the mathematical model,
not arbitrary token calls or full EVM success.

The initial stored reserves must also reconcile with `ERC20.balanceOf(Pool)`
minus pending-deposit escrow and global treasury/partner fee buckets. A token
donation can predate the snapshot without emitting Pool `Sync`; the next swap's
`Sync` would then see a reserve change absent from the cached stored-reserve
model. Reconcile that partition externally before claiming exact transitions.
The math APIs do not fetch raw balances or LP pending-deposit state.

Paused/stale results have empty ladders and zero coverage counters, never
`exhaustiveLotPolicy` safety. `Active` alone only describes the freshness gate.

## Multilevel precision fitting

`try_build_precise_order_book(state, config, precision)` / Node
`buildPreciseOrderBook(snapshot, config, precision)` retain the finite model's
cursor and direction-interleaving constraints while fitting up to 20 levels.
They use the same fee-accounting preflight, enforced fill policy, snapshot and
paired-generation assumptions described above. Neither the indicative builder
nor the existing one-level validated builder changes behavior.

```ts
const precise = buildPreciseOrderBook(snapshot, config, {
  maxLevels: 20,
  targetUnderquoteBps: 10,
  maxWork: 1_000_000,
});
const { book } = precise.validated;
console.log(precise.targetMet, precise.worstUnderquoteBps,
  precise.worstFreshSnapshotDiscountBps);
// Do not publish at the requested tolerance unless precise.targetMet is true.
```

The target describes the gap to the **minimum output over all reachable states
for the same direction, cursor and amount**. This separates price fitting from
the genuine reduction in output caused by later punishment and mixed-direction
execution. `worstUnderquoteBps` measures that fitting gap;
`worstFreshSnapshotDiscountBps` separately compares each promised output with
the original snapshot's same-size quote. Both report ceiling-rounded bps,
clamping a negative discount to zero. A zero fitting gap can coexist with a
positive fresh-snapshot discount.

`targetMet` is an observed result, not an unconditional tolerance guarantee.
The bounded fitting heuristic can miss the target without proving that every
possible ladder is infeasible. Such a result stays conservative and explicitly
reports `targetMet: false`; gate publication on that field when enforcing the
requested tolerance. Inactive results also report false. `maxLevels` accepts
1..=20 and `targetUnderquoteBps` 0..=10000.
`maxWork` is a separate fitting budget in 1..=5,000,000; `maxTransitions` still
bounds reachable-state exploration. Either exhausted budget returns an error,
never an unchecked partial certificate. The result includes `workUsed`,
`constraintCount`, its input `precision` and the full `validated` certificate.

Candidate scores compare dimensionless relative gaps at 1e-18 resolution,
preventing raw token decimals from biasing the fit. The certificate applies to
the exact returned raw levels: revalidate executor packing, quantization,
boundary rounding or merging. Removing even an equal-price boundary removes a
tranche floor and can increase the output promised by the transformed ladder.

## Indicative sampled conversion

For cumulative sizes `s[i]`, the builder obtains `q[i]` from the existing
bit-exact Pool quote without mutating the snapshot. It then emits:

```text
price[i] = floor((q[i] - q[i-1]) * 1e18 / (s[i] - s[i-1]))
```

Prices are clamped so they never improve with depth. The per-tranche floor
makes the swept ladder output no greater than the Pool quote at every emitted
cumulative prefix. All samples use the same Pool state; chaining swap
simulations between rungs would model order splitting and can over-promise a
single large fill.

`try_ladder_amount_out` and `try_ladder_amount_out_at_cursor` (Node:
`ladderAmountOut(levels, amountIn, cursor?)`) return the exact sum of
`floor(consumedInput * price / 1e18)` across consumed tranches. An execution
system consuming these raw levels must preserve that exact amount; there must
be no lossy round-trip through a volume-weighted average price.
For example, input 3 at price `0.5e18` produces output 1, not 0. Requests beyond
depth return `None`/`null`.

This prefix property is narrower than a general execution guarantee. The Pool
uses two nested Solidity floors and a ceil-rounded immediate punishment, so its
integer quote is not a continuous concave curve. A fill inside a rung, a later
fill starting from the ladder's consumed lifetime cursor, or a state change after
signing can differ by rounding or new punishment. Production integration must:

1. use short expiries and rebuild after every relevant Pool state change;
2. enforce execution lots in the adapter and use the exhaustively validated
   builder when a modeled fill guarantee is required;
3. have the external adapter call the Pool with the exact ladder `amountOut` as
   `amountOutMinimum`, so an under-delivery reverts instead of spending adapter
   inventory;
4. keep the signed cap below operational risk limits even though the builder
   also checks Solidity reserve bounds.

Each protocol-specific adapter is a settlement/safety boundary, not part of this
pure library. Every result exposes `requiresAmountOutMinimum: true` so a
publisher cannot mistake a directional ladder projection for a guarantee against
unmodeled external state changes. A sampled projection's `safety` stays
`indicative`. Sample grids must be positive and strictly increasing, with at
most 20 levels. Empty grids disable a side; exhausted reserve headroom/output,
zero prices or non-increasing quotes truncate it and set `truncated: true`.

## Event-backed snapshot contract

The cache engine is external. The builder assumes its input is one coherent,
canonical snapshot. A log subscriber should bootstrap from a complete
block-pinned read, retain the block hash for reorg handling, and apply all logs
from a transaction/block atomically before exposing a new snapshot.

Relevant state changes are:

| Event/input | Cached fields |
| --- | --- |
| `StateUpdated` | anchor, ask, bid; `latestUpdateBlock = log.blockNumber` |
| `PunishmentApplied` | absolute ask and bid from the event |
| `Sync` | both active reserves |
| `MaxPunishmentX24Set` | maximum punishment |
| `BlockDelaySet` | freshness window |
| `Paused` / `Unpaused` | pause state |
| `WhitelistSet(adapter, ...)` | adapter whitelist state |
| `BlacklistFeeMultiplierSet` | non-whitelisted multiplier |
| `PartnerFeeSet(adapter, ...)` / `PartnerOperatorSet(adapter, ...)` | actual adapter's partner share/operator |
| swap fee credits / `PartnerFeeTaken` | treasury, global partner, and adapter cumulative fee buckets |
| `PartnerFeesWithdrawn` / `TreasuryFeesWithdrawn` | global and per-router fee bucket decreases |
| ERC-1967 `Upgraded` | halt publication; verify the new implementation and bootstrap again |
| each canonical new head | `snapshotBlock`, even when the Pool emitted no log |

Freshness is checked at mandatory `maxExecutionBlock`, not merely at
`snapshotBlock` or the first possible fill block. It is the conservative last
block covered by the signed `expiresAt` policy. The builder rejects a bound
before the snapshot and returns `Stale` when
`maxExecutionBlock >= latestUpdateBlock + blockDelay`. If the publisher cannot
bound the block range of its wall-clock TTL, it must not treat this preflight as
an execution guarantee; the adapter still fails closed on Pool freshness.

`PunishmentApplied` and `Sync` occur within a successful swap transaction. Do
not publish the intermediate fee-only state. A `Sync` changes the punishment
denominator for both directions, so both ladders must be rebuilt.
Fee-accounting values must also reflect the entire block. If event payloads do
not provide sufficient information to reconstruct every global fee bucket,
refresh those views at the pinned block hash before exposing the snapshot.

The fee multiplier must be derived for the exact address that will call
`Pool.swapExactIn` (normally the external adapter). It is not determined by the
taker, quote signer, or output recipient. `maxPunishmentX24` and this
resolved multiplier are mandatory inputs: the builder never substitutes a
more favorable default for a missing cached value.

On a disconnect, log gap, removed log, or reorg, mark the snapshot unhealthy
and rebuild it from canonical block-pinned views before publishing again. Cache
health and block hash are deliberately not accepted as booleans by the math
API: a caller must never turn an unhealthy cache record into an apparently
active book.

Treat a UUPS/ERC-1967 `Upgraded` event as a hard compatibility boundary rather
than an ordinary state update. Stop publishing, verify or allowlist the new
implementation and its quote semantics, then perform a complete block-pinned
bootstrap before resuming.
