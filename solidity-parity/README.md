# Portable Solidity parity oracle

This Foundry project imports the production `Pool` and `SwapLib` from a
separate `dark_pools` checkout. It intentionally does not vendor or modify the
Solidity source it treats as the canonical oracle.

Generate schema-v4 JSONL corpora and replay them in every language package:

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
`Pool.swapExactIn`. `effectiveFeeX24` is the saturating directional fee used by
the current quote before the caller multiplier. A non-`Applied` outcome
represents an atomic rollback and keeps both fees and reserves at their
pre-swap values, even though the quote exposes the counterfactual immediate fee
and punishment.

`MathMulDivRevert` means quote evaluation never completed. Those rows encode
`desiredPunishmentX24` and `effectiveFeeX24` as `"0"`/unavailable and retain the
entire pre-state.
