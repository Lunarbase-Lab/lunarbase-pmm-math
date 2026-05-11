# Offchain quoting examples

End-to-end Rust reference for partners who want to quote against a LunarBase
Pool **off-chain** with sub-block latency. The example connects to a Base
[flashblocks](https://docs.base.org/) node, mirrors the on-chain pool state in
Redis, and computes quotes through [`lunarbase-pmm-math`](../../math/rust/lunarbase-pmm-math)
— the same math the on-chain contract uses, bit-for-bit.

> The examples in this directory only depend on **public** contract views,
> events, and the public `lunarbase-pmm-math` crate. There is nothing in here
> about how the operator publishes the anchor price; partners do not need to
> reproduce that — they just consume `StateUpdated` events.

## Layout

| Path                           | Crate                                  | Targets                                                                                    |
| ------------------------------ | -------------------------------------- | ------------------------------------------------------------------------------------------ |
| [`rust/`](rust/)               | `offchain-quoting-example-rust`        | the **current** Pool ABI on `fix/incident` (`uint160 anchorPrice` Q96, single price)        |
| [`rust-legacy/`](rust-legacy/) | `offchain-quoting-example-rust-legacy` | the **legacy** Pool ABI (`uint160 pX96`, no `anchorPrice()` view, single Q48 fee)           |

Both crates are members of the workspace and share the same module layout.
Pick whichever matches the contract version you are integrating against.

## What the example does

1. **Seed** initial pool state from an HTTP RPC (`getXReserve`, `getYReserve`,
   `state()`, `anchorPrice()`, `concentrationK()`, `blockDelay()`, `paused()`).
2. **Subscribe** over WebSocket to three streams:
   - `newHeads` — confirmed block tip (drives `blockAge` / freshness).
   - `newFlashblocks` — Base pre-confirmation block updates (~200 ms cadence).
   - `pendingLogs` filtered by the pool address — pending event logs from the
     contract before the block is finalized.
3. **Apply contract events** to the cached pool state in Redis:
   - `Sync(reserveX, reserveY)` — atomically replaces cached reserves.
   - `SwapExecuted(recipient, xToY, dx, dy, fee)` — locally projects the swap
     through `lunarbase_pmm_math::quote_x_to_y` / `quote_y_to_x` and updates
     cached reserves by the gross deltas. Sanity-checks the local `fee`
     against the on-chain `fee`. The local `sqrtPriceNext` is informational —
     on `fix/incident` actual on-chain `sqrtPriceX96` is operator-only.
   - `StateUpdated(anchorPrice, feeAskX24, feeBidX24)` — refreshes the
     operator-published `sqrtPriceX96` (Q64.96) and per-direction fees. The
     legacy variant additionally consolidates the single-fee path.
   - `ConcentrationKSet`, `BlockDelaySet`, `Paused`, `Unpaused` — cached.
4. **Deduplicate** logs that the flashblocks node re-emits across pre-confirmation
   snapshots: the dedup key is `(blockNumber, transactionHash, logIndex)`,
   falling back to `keccak256(blockNumber ‖ topic0 ‖ data)` when the node
   omits a transaction hash on pending logs.
5. **Log every applied event to stdout.** The price line on each
   `SwapExecuted` is the live quote signal for partner consumers.

## Running

### 1. Local Redis

```sh
docker run -d --name lunarbase-redis -p 6379:6379 redis:7-alpine
```

### 2. Configure (env, all optional — sane defaults are baked in)

| Variable       | Default                                    | Notes                                                               |
| -------------- | ------------------------------------------ | ------------------------------------------------------------------- |
| `POOL_ADDRESS` | mainnet ETH/USDC pool                      | Pool contract address.                                              |
| `RPC_URL`      | preconfigured Base node                    | HTTP RPC for the one-shot seed (`state()`, reserves, params).       |
| `FLASH_WS`     | preconfigured Base flashblocks WS          | WebSocket source for `newHeads` + `newFlashblocks` + `pendingLogs`. |
| `REDIS_URL`    | `redis://127.0.0.1:6379`                   | Connection string for Redis.                                        |
| `RUST_LOG`     | `info,offchain_quoting_example_rust=debug` | Standard `tracing-subscriber` env filter.                           |

### 3. Run

Current ABI:

```sh
cargo run --release -p offchain-quoting-example-rust
```

Legacy ABI:

```sh
cargo run --release -p offchain-quoting-example-rust-legacy
```

You should see, in roughly this order:

```text
INFO starting offchain quoter pool=0x... rpc=... ws=... redis=...
INFO seeded pool state from RPC head_block=... reserve_x=... reserve_y=... ...
INFO opening flashblocks WS ws_url=...
INFO StateUpdated  block=... anchor_price=<Q96> fee_ask_x24=... fee_bid_x24=...
INFO Sync          block=... reserve_x=... reserve_y=...
INFO swap applied; ...  direction="X->Y" dx=... dy=... sqrt_price_x96=<Q96> reserve_x=... reserve_y=...
```

## Inspecting the cache

The Redis key layout (per pool) follows the same shape as a production
deployment so that your service code translates 1:1 to a partner-side worker:

| Key                           | Type          | TTL  | Content                                           |
| ----------------------------- | ------------- | ---- | ------------------------------------------------- |
| `reserves:<pool>`             | string (JSON) | 10 s | `["<reserveX>", "<reserveY>"]`                    |
| `updates:<pool>`              | string (JSON) | 6 s  | `{"block": N, "anchorPrice": "<Q96>", "feeAskX24": N, "feeBidX24": N}` |
| `sqrtprice:<pool>`            | string        | 6 s  | decimal `sqrt_price_x96` (Q64.96)                 |
| `pmm:concentrationK:<pool>`   | string        | 60 s | decimal `uint32`                                  |
| `pmm:blockDelay:<pool>`       | string        | 60 s | decimal `uint48`                                  |
| `pmm:paused:<pool>`           | string        | 60 s | `0` / `1`                                         |
| `head:<pool>`                 | string        | 30 s | confirmed `blockNumber`                           |
| `log:tx:<pool>:<fingerprint>` | string        | 10 s | dedup token (`SET NX EX 10`)                      |

```sh
docker exec lunarbase-redis redis-cli MONITOR
docker exec lunarbase-redis redis-cli KEYS '*'
```

## How latency is achieved

- The **WebSocket reader task** does I/O + lightweight JSON parsing and forwards
  routed `ChainEvent`s through a bounded `mpsc(1024)` channel.
- A **single event consumer** owns the `Cache` (no `Mutex`) and applies events
  in arrival order, which is the only correct semantics for `Sync` /
  `SwapExecuted` / `StateUpdated` interleavings within one block.
- A **backpressure monitor** logs a `WARN` every 5 s if the channel high-water
  mark exceeds 75 % — that's the signal that Redis (or your handler) is
  lagging.
- Swap-delta application uses an atomic Redis `MULTI` pipeline (`sqrtprice` +
  `reserves` written together) so consumers never observe a torn post-swap
  state.

End-to-end latency from `pendingLogs` → `swap applied` Redis write is single-digit
milliseconds in the example deployment.

## What is **not** in here

- No anchor-price computation, no volatility model, no CEX integration.
  Partners should not — and do not need to — reproduce that side of the
  system. They consume `StateUpdated` events, which is the operator's
  agreed-upon public interface.
- No write-side: no transaction signing, no `swapExactIn` calldata
  construction. The example is read-only.

## Reference

- Math crate: [`math/rust/lunarbase-pmm-math`](../../math/rust/lunarbase-pmm-math)
- Public quoter API: `quote_x_to_y(params, dx)`, `quote_y_to_x(params, dy)`
- Pool params struct: `PoolParams { sqrt_price_x96, fee_ask_x24, fee_bid_x24, reserve_x, reserve_y, concentration_k }`
