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

The current example is the integration target.

## What it does

1. **Seed** initial pool state from an HTTP RPC. This is the only required
   synchronous chain read before the service can quote from cache.
2. **Subscribe** over WebSocket to `newHeads`, `newFlashblocks`, and
   `pendingLogs` filtered by the pool address.
3. **Apply authoritative state events** (`Sync`, `StateUpdated`,
   `ConcentrationKSet`, `BlockDelaySet`, `Paused`/`Unpaused`,
   `WhitelistSet`, `BlacklistFeeMultiplierSet`) to the cache. Current pools
   emit `Sync` before `SwapExecuted`, so the latter is observed for fills only
   and is never applied to reserves a second time.
4. **Deduplicate** logs re-emitted across pre-confirmation snapshots by
   `(blockNumber, transactionHash, logIndex)`.
5. **Quote from Redis only** through `quote_exact_in`. The quoter reads
   anchor price, directional fees, reserves, concentration, freshness config,
   paused state, and the effective fee multiplier for the configured execution
   caller. A quote's hypothetical `pNext` is returned but not cached as current
   Pool state.

End-to-end latency from `pendingLogs` → Redis write is single-digit
milliseconds in the example deployment.

## Data needed for a fully off-chain quote

The math crate does not call the chain. Your service must keep these values
warm in cache:

| Value | Source | Why it is needed |
| ----- | ------ | ---------------- |
| `anchorPrice` / `sqrt_price_x96` | `state()` + `StateUpdated` | PMM anchor in Q64.96 |
| `feeAskX24`, `feeBidX24` | `state()` + `StateUpdated` | Directional base fees |
| `reserveX`, `reserveY` | `getXReserve`, `getYReserve` + `Sync` | Current cached reserves |
| `concentrationK` | `concentrationK()` + `ConcentrationKSet` | Curve concentration in Q20.12 |
| `blockDelay`, `latestUpdateBlock`, `head` | `blockDelay()`, `state()`, `newHeads` | Fail-closed freshness check |
| `paused` | `paused()` + `Paused`/`Unpaused` | Do not quote executable swaps while paused |
| `isWhitelisted(QUOTE_CALLER_ADDRESS)` | seed + `WhitelistSet` | Decides whether multiplier is `1` |
| `blacklistFeeMultiplier` | seed + `BlacklistFeeMultiplierSet` | Multiplier for non-whitelisted callers |

`QUOTE_CALLER_ADDRESS` must be the exact address the Pool sees as
`msg.sender`: router, execution adapter, proxy, or settlement contract. It is
not the taker EOA and not `SwapExecuted.recipient`. If you omit `from` in
`eth_call`, many tools simulate as `address(0)` and therefore hit a different
fee path than production.

## Running

### Docker Compose

Run Redis and the Rust quoter together:

```sh
cd examples/offchain-quoting
cp rust/.env.example rust/.env
# edit rust/.env and set RPC_URL / FLASH_WS
docker compose up --build
```

Useful commands:

```sh
docker compose logs -f quoter
docker compose exec redis redis-cli MONITOR
docker compose down
```

The compose file reads `RPC_URL` and `FLASH_WS` from `rust/.env`. Keep
private/internal endpoints there only; `.env` is git-ignored.

The compose file defaults only non-sensitive values:

```sh
POOL_ADDRESS=0x0000eFC4ec03a7c47D3a38A9Be7Ff1d52dD01b99
QUOTE_CALLER_ADDRESS=0x0000000000000000000000000000000000000000
```

Override values inline when needed:

```sh
QUOTE_CALLER_ADDRESS=0xYourRouterOrExecutionAdapter \
QUOTE_AMOUNT_IN=1000000000000000 \
QUOTE_DIRECTION=x_to_y \
QUOTER_RUST_LOG=info,offchain_quoting_example_rust=debug \
docker compose up --build
```

Inside compose, `REDIS_URL` is set to `redis://redis:6379`; do not point it at
`127.0.0.1`, because the service runs in a separate container.

### Local

```sh
# 1. local Redis
docker run -d --name lunarbase-redis -p 6379:6379 redis:7-alpine

# 2. copy and edit config
cp examples/offchain-quoting/rust/.env.example examples/offchain-quoting/rust/.env

# 3. run the current contract example
cargo run --release -p offchain-quoting-example-rust
```

Configurable via env: `POOL_ADDRESS`, `RPC_URL`, `FLASH_WS`, `REDIS_URL`,
`QUOTE_CALLER_ADDRESS`, `QUOTE_AMOUNT_IN`, `QUOTE_DIRECTION`,
`QUOTE_INTERVAL_SECS`, `SEED_TIMEOUT_SECS`, `REDIS_CONNECT_TIMEOUT_SECS`,
`RUST_LOG`.

You can also run without a `.env` file:

```sh
POOL_ADDRESS=0x0000eFC4ec03a7c47D3a38A9Be7Ff1d52dD01b99 \
QUOTE_CALLER_ADDRESS=0x0000000000000000000000000000000000000000 \
RPC_URL=<http-rpc-url> \
FLASH_WS=<websocket-url> \
REDIS_URL=redis://127.0.0.1:6379 \
QUOTE_AMOUNT_IN=1000000000000000 \
QUOTE_DIRECTION=x_to_y \
cargo run --release -p offchain-quoting-example-rust
```

## Redis layout

| Key                           | Type   | TTL  | Content                                   |
| ----------------------------- | ------ | ---- | ----------------------------------------- |
| `reserves:<pool>`             | JSON   | —    | `["<reserveX>", "<reserveY>"]`            |
| `updates:<pool>`              | JSON   | —    | `{block, anchorPrice, feeAskX24, feeBidX24}` |
| `pmm:concentrationK:<pool>`   | string | —    | decimal `uint32`                          |
| `pmm:blockDelay:<pool>`       | string | —    | decimal `uint48`                          |
| `pmm:paused:<pool>`           | string | —    | `0` / `1`                                 |
| `pmm:callerWhitelisted:<pool>:<caller>` | string | — | `0` / `1` for the configured caller |
| `pmm:blacklistFeeMultiplier:<pool>` | string | — | decimal `uint256`                         |
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

The example uses separate Redis connections for event ingestion and quote
reads. Keep that separation in production so slow client requests cannot block
chain-event processing. For multiple execution callers, run one quoter cache
namespace per caller or include caller in the fee-policy keys as shown here.

See [`math/rust/lunarbase-pmm-math`](../../math/rust/lunarbase-pmm-math) for
the quoter API.
