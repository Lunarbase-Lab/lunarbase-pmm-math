# Offchain quoting examples

End-to-end Rust reference for partners who want to quote against a LunarBase
Pool **off-chain** with sub-block latency. The example connects to a Base
[flashblocks](https://docs.base.org/) node, mirrors the on-chain pool state
in Redis, and computes quotes through
[`lunarbase-pmm-math`](../../math/rust/lunarbase-pmm-math) — bit-for-bit
identical with the current on-chain math for the effective fee multiplier and
the crate's supported `u128` Q96 anchor range.

> Only depends on **public** contract views, events, and the public
> `lunarbase-pmm-math` crate. Partners consume `StateUpdated` events; they do
> not reproduce the operator's anchor-price computation.

## Layout

| Path                           | Crate                                  | Targets                                          |
| ------------------------------ | -------------------------------------- | ------------------------------------------------ |
| [`rust/`](rust/)               | `offchain-quoting-example-rust`        | Current Pool ABI (`anchorPrice` Q96, asym fees)  |
| [`rust-legacy/`](rust-legacy/) | `offchain-quoting-example-rust-legacy` | Approximate migration aid for the legacy ABI     |

Pick whichever matches the contract version you are integrating against.
The legacy example truncates its single Q48 fee into the current directional
Q24 API and is therefore not a bit-exact `0.2.5` reference.

## What it does

1. **Seed** initial pool state from an HTTP RPC.
2. **Subscribe** over WebSocket to `newHeads`, `newFlashblocks`, and
   `pendingLogs` filtered by the pool address.
3. **Apply authoritative state events** (`Sync`, `StateUpdated`,
   `ConcentrationKSet`, `BlockDelaySet`, `Paused`/`Unpaused`) to the cache.
   Current pools emit `Sync` before `SwapExecuted`, so the latter is observed
   for fills only and is never applied to reserves a second time.
4. **Deduplicate** logs re-emitted across pre-confirmation snapshots by
   `(blockNumber, transactionHash, logIndex)`.
5. **Quote from the latest anchor + reserves** through
   `quote_exact_in_whitelisted`, with `fee_multiplier = 1` for the known
   whitelisted aggregator/execution-adapter path. `SwapExecuted.recipient` is
   not the caller and must never be used to infer fee class. A quote's
   hypothetical `pNext` is returned but not cached as current Pool state.

End-to-end latency from `pendingLogs` → Redis write is single-digit
milliseconds in the example deployment.

## Running

```sh
# 1. local Redis
docker run -d --name lunarbase-redis -p 6379:6379 redis:7-alpine

# 2. run (env vars all optional — sane defaults baked in)
cargo run --release -p offchain-quoting-example-rust
cargo run --release -p offchain-quoting-example-rust-legacy
```

Configurable via env: `POOL_ADDRESS`, `RPC_URL`, `FLASH_WS`, `REDIS_URL`,
`RUST_LOG`.

## Redis layout

| Key                           | Type   | TTL  | Content                                   |
| ----------------------------- | ------ | ---- | ----------------------------------------- |
| `reserves:<pool>`             | JSON   | —    | `["<reserveX>", "<reserveY>"]`            |
| `updates:<pool>`              | JSON   | —    | `{block, anchorPrice, feeAskX24, feeBidX24}` |
| `sqrtprice:<pool>`            | string | —    | legacy-only swap-driven Q64.96 sqrt-price |
| `pmm:concentrationK:<pool>`   | string | —    | decimal `uint32`                          |
| `pmm:blockDelay:<pool>`       | string | —    | decimal `uint48`                          |
| `pmm:paused:<pool>`           | string | —    | `0` / `1`                                 |
| `head:<pool>`                 | string | 30 s | confirmed `blockNumber`                   |
| `log:tx:<pool>:<fingerprint>` | string | 10 s | dedup token (`SET NX EX 10`)              |

Inspect:

```sh
docker exec lunarbase-redis redis-cli MONITOR
docker exec lunarbase-redis redis-cli KEYS '*'
```

## Scope

The example is **read-only**: no transaction signing, no swap calldata
construction, no anchor-price computation, no CEX integration.

Required quote state is persistent so a quiet pool remains quoteable between
swaps. The expiring `head` key is a separate liveness guard: if head updates
stop, the quoter fails closed instead of presenting old state as fresh.

The current example is the integration target. The legacy example preserves
the old swap-driven sqrt-price behavior only as an approximate migration aid.

See [`math/rust/lunarbase-pmm-math`](../../math/rust/lunarbase-pmm-math) for
the quoter API.
