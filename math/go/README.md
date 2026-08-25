# Go math mirror

`lunarbasepmm` mirrors the Solidity `SwapLib` linear-anchor quote and immediate
directional-punishment state transition.

## State and numeric domains

`PoolParams` uses `*uint256.Int` for the Q64.96 anchor and reserves so the
complete Solidity domains remain available. `ValidatePoolParams` enforces:

- `SqrtPriceX96`: `uint160`
- `FeeAskX24`, `FeeBidX24`, `MaxPunishmentX24`: `uint24`
- `ReserveX`, `ReserveY`: `uint112`

`MaxUint24` is a sentinel for conceptual `Q24Scale` (100%) for both fees and
maximum punishment.

## Quotes

```go
result := lunarbasepmm.QuoteXToY(pool, amountIn)
result = lunarbasepmm.QuoteXToYWithMultiplier(pool, amountIn, multiplier)
```

X -> Y uses the bid fee; Y -> X uses the ask fee. Price conversion deliberately
keeps Solidity's two floors:

```text
X -> Y: floor(floor(amountIn * anchor / Q96) * anchor / Q96)
Y -> X: floor(floor(amountIn * Q96 / anchor) * Q96 / anchor)
```

Punishment is computed first and saturating-added to the stored directional
fee. The gross result is then checked against the output active reserve before
that effective fee is charged. `EffectiveFeeX24` exposes the fee used before
the caller multiplier, and `SqrtPriceNext` always equals the anchor. A stored
fee of `MaxUint24` consumes
the entire gross output. Multiplier overflow or a scaled fee greater than or
equal to gross output has the same full-fee result as Solidity.

The ergonomic quote functions panic on malformed off-chain domains or a
Solidity-equivalent arithmetic revert. The `*Checked` variants return those
conditions as Go errors. `*Into` and `*IntoChecked` reuse preallocated result
fields and keep the valid hot path allocation-free.

## Punishment and state transition

`DesiredPunishmentX24` uses pre-swap active reserves and mirrors Solidity's
ceil rounding:

```text
inventoryY = floor(floor(reserveX * anchor / Q96) * anchor / Q96) + reserveY
swapY      = X->Y ? floor(floor(amountIn * anchor / Q96) * anchor / Q96)
                   : amountIn
desired    = ceil(effectiveMax * min(swapY, inventoryY) / inventoryY)
```

`TransitionDirectionalFees` performs the saturating add without mutating a
pool. `ApplyDirectionalPunishment` commits just that fee transition.
`ApplyStateUpdate` replaces the anchor and both effective fees, resetting
accumulated punishment while preserving reserves and `MaxPunishmentX24`.

`SimulateStandardTokenSwap` / `SimulateStandardTokenSwapWithMultiplier` compute
desired punishment from pre-swap reserves, charge the resulting effective fee
on the current quote, and atomically commit that directional fee together with
the standard-token active reserve transition. X -> Y adds input to X and
subtracts `AmountOut + Fee` from active
Y; Y -> X is symmetric. Any quote, math, impossible-swap, or uint112 reserve
failure leaves `PoolParams` unchanged.

`SwapImpossible` and `ReserveTransitionOverflow` are completed, structured
simulation outcomes rather than Go errors. The returned `SwapSimulationResult`
has `Executable == false`, the matching `Status`, zero
`AppliedPunishmentX24`, and preserves the counterfactual quote,
`EffectiveFeeX24`, and `DesiredPunishmentX24`. Arithmetic and invalid-domain
failures still return errors. `MarkRolledBack(RollbackReasonLaterRevert)` can
restore an applied result when a later token transfer or accounting step fails;
it refuses to overwrite `PoolParams` that changed after the simulated commit.

Quote helpers are stateless. Repeating a quote against unchanged `PoolParams`
does **not** model a sequential split swap: every call sees the same stored fee
and reserves. To model split execution, chain successful
`SimulateStandardTokenSwap*` calls against the mutated `PoolParams`.

The mutable simulation APIs require the anchor and reserve pointers to be
distinct, and their result fields must not alias the state or swap inputs.
This keeps the pre-swap snapshot stable until the atomic commit. Checked Into
calls clear structurally valid result objects on math or domain errors, so a
reused output never exposes stale fields from an earlier quote.

## Tests

`go test ./...` includes deterministic BigInt property suites and replays the
same 49 deterministic plus 10,000 seeded-fuzz Solidity rows as Rust and Node.
Quote output, fee, desired/applied punishment, directional fees, reserves, and
rollback status must match bit-for-bit.
