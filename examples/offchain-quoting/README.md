# Offchain quoting examples

End-to-end Rust reference for partners who want to quote against a LunarBase
Pool **off-chain** with sub-block latency. The example connects to a Base
[flashblocks](https://docs.base.org/) node, mirrors the on-chain pool state
in Redis, and computes quotes through
[`lunarbase-pmm-math`](../../math/rust/lunarbase-pmm-math) — bit-for-bit
identical with the on-chain contract.

> Only depends on **public** contract views, events, and the public
> `lunarbase-pmm-math` crate. Partners consume `StateUpdated` events; they do
> not reproduce the operator's anchor-price computation.

## Layout

| Path                           | Crate                                  | Targets                                          |
| ------------------------------ | -------------------------------------- | ------------------------------------------------ |
| [`rust/`](rust/)               | `offchain-quoting-example-rust`        | Current Pool ABI (`anchorPrice` Q96, asym fees)  |
| [`rust-legacy/`](rust-legacy/) | `offchain-quoting-example-rust-legacy` | Legacy Pool ABI (`pX96`, single Q48 fee)         |

Pick whichever matches the contract version you are integrating against.

## What it does

1. **Seed** initial pool state from an HTTP RPC.
2. **Subscribe** over WebSocket to `newHeads`, `newFlashblocks`, and
   `pendingLogs` filtered by the pool address.
3. **Apply contract events** (`Sync`, `SwapExecuted`, `StateUpdated`,
   `ConcentrationKSet`, `BlockDelaySet`, `Paused`/`Unpaused`) to the cached
   pool state.
4. **Deduplicate** logs re-emitted across pre-confirmation snapshots by
   `(blockNumber, transactionHash, logIndex)`.
5. **Log every applied event** to stdout — the price line on `SwapExecuted`
   is the live quote signal.

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
| `reserves:<pool>`             | JSON   | 10 s | `["<reserveX>", "<reserveY>"]`            |
| `updates:<pool>`              | JSON   | 6 s  | `{block, anchorPrice, feeAskX24, feeBidX24}` |
| `sqrtprice:<pool>`            | string | 6 s  | decimal Q64.96 sqrt-price                 |
| `pmm:concentrationK:<pool>`   | string | 60 s | decimal `uint32`                          |
| `pmm:blockDelay:<pool>`       | string | 60 s | decimal `uint48`                          |
| `pmm:paused:<pool>`           | string | 60 s | `0` / `1`                                 |
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

See [`math/rust/lunarbase-pmm-math`](../../math/rust/lunarbase-pmm-math) for
the quoter API.
