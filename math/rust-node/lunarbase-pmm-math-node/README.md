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
  priceToSqrtPriceX96,
  quoteXToY,
  simulateXToY,
  SwapSimulationStatus,
  type QuoteParams,
} from "@lunarbase-lab/pmm-math";

const params: QuoteParams = {
  sqrtPriceX96: priceToSqrtPriceX96(1),
  feeAskX24: 0,
  feeBidX24: 50_331,
  reserveX: "1000000000000000000000",
  reserveY: "1000000000000000000000",
  maxPunishmentX24: 1_677_721, // about 10% maximum increment
  amountIn: "1000000000000000000",
  feeMultiplier: "1",
};

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
| `priceToSqrtPriceX96(price)`, `sqrtPriceX96ToPrice(p)` | Decimal price and Q64.96 anchor helpers |
| snake_case price helpers | Compatibility aliases |

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
