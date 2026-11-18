# Portable Solidity parity oracle

This Foundry project imports the production `Pool` and `SwapLib` from a
separate clean `dark_pools` checkout pinned to `update/stale-price` at
`7083e2b1ddb8aeef51f5881bf8d2431f0cf6217d`. Generation copies it to a disposable
directory and applies `active-treasury.patch`, which changes only the two reserve
sync expressions in `PoolBridgeLib`. The input checkout remains untouched.
The manifest records hashes of the base and patched bridge, the patch, and
the existing quote sources, so the combined 0.5.0 model is reproducible.

Generate schema-v6 JSONL corpora and replay them in every language package:

```sh
DARK_POOLS_DIR=/path/to/dark_pools ./scripts/regenerate-vectors.sh
```

The first positional argument is accepted instead of `DARK_POOLS_DIR`. The
path may point to either the `dark_pools` repository or its `contracts`
directory. The script asks that checkout for its Foundry remappings and makes
all remapping targets absolute before compiling this project.

The default seed and 5,000 fuzz runs produce exactly 10,000 fuzz rows: one
`xToY` and one `yToX` transition for every run. A larger reproducible corpus is
selected explicitly:

```sh
DARK_POOLS_DIR=/path/to/dark_pools \
PARITY_FUZZ_RUNS=10000 \
PARITY_FUZZ_SEED=0x706d6d2d7061726974792d763200000000000000000000000000000000000001 \
./scripts/regenerate-vectors.sh
```

For a generator-only smoke test, set `PARITY_SKIP_REPLAY_TESTS=1`. Generated
scratch files live below `solidity-parity/generated/`; the script then copies
the canonical corpora to the Rust and Go fixture directories. Node.js replays
the Rust fixture paths.

[`vector-manifest.json`](vector-manifest.json) locks the Solidity source
commit and source-file hashes together with the fuzz seed, row counts, and
corpus hashes. `solidityOracleDirty` makes vectors generated from local
contract edits explicit.

Every row records the triggering quote after its amount-dependent punishment
has been saturating-added to the stored directional fee, the desired pure
`SwapLib.punishmentX24` result, and the observed state from a real
`Pool.swapExactIn`. Schema v6 retains the maximum age multiplier and coherent current/update/delay
blocks. Deterministic rows include cubic punishment, age golden values,
rounding, saturation, the inclusive last-fresh block, and stale rollbacks;
fuzz rows vary age and the uint64 maximum. `effectiveFeeX24` is the saturating
directional fee before the age and caller multipliers. A non-`Applied` outcome
represents an atomic rollback and keeps both fees and reserves at their
pre-swap values, even though the quote exposes the counterfactual immediate fee
and punishment.

`MathMulDivRevert` means quote evaluation never completed. Those rows encode
`desiredPunishmentX24` and `effectiveFeeX24` as `"0"`/unavailable and retain the
entire pre-state.

`StalePrice` rows return zero output and token fee without punishment math; the
effective-fee field carries the unchanged stored directional fee.

Schema v6 records active-treasury reserve transitions. The extra
`ActiveTreasuryParity` suite fuzzes partner shares, missing operators, and both
swap directions against the real Pool (5,000 runs by default).
