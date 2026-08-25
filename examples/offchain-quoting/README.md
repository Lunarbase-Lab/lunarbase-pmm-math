# Offchain quoting examples

End-to-end Rust reference for partners who want to quote against a LunarBase
Pool **off-chain** from internally consistent confirmed-head snapshots. The
example uses WebSocket heads to trigger hash-pinned HTTP reads, publishes one
atomic caller-specific snapshot in Redis, and computes quotes through
[`lunarbase-pmm-math`](../../math/rust/lunarbase-pmm-math) — bit-for-bit
identical with the current on-chain math for the effective fee multiplier and
the full Solidity `uint160` Q96 anchor range.

> Only depends on **public** contract views, `newHeads`, and the public
> `lunarbase-pmm-math` crate. It does not reproduce the operator's anchor-price
> computation and never applies pending logs to quote state.

## Layout

| Path                           | Crate                                  | Targets                                          |
| ------------------------------ | -------------------------------------- | ------------------------------------------------ |
| [`rust/`](rust/)               | `offchain-quoting-example-rust`        | Current Pool ABI (`anchorPrice` Q96, asym fees)  |

The current example is the integration target.

## What it does

1. **Mark Redis unhealthy**, then open the WebSocket and subscribe to
   `newHeads`. No initial RPC snapshot is allowed before that subscription is
   acknowledged. Acknowledgement alone does not trigger HTTP reads: the quoter
   waits for the first valid head containing both `number` and `hash`.
2. For every confirmed `newHeads` notification, **mark the cache unhealthy
   before RPC work**, resolve the observed hash over HTTP, require its block
   number to match the WS notification, and pin every contract view to that
   hash using EIP-1898 with `requireCanonical=true`.
3. **Publish atomically**: the complete pool/caller JSON snapshot and a
   short-lived synchronized lease become visible in one Redis transaction.
   Any timeout, RPC error, disconnect, or malformed snapshot leaves quotes
   disabled. Redis connection epochs plus a generation token prevent queued or
   in-flight heads from an old WebSocket connection from republishing health
   after a disconnect.
4. **Quote from Redis only** through `quote_exact_in`. The quoter refuses an
   unhealthy/missing snapshot, validates Solidity numeric widths, checks
   pause/freshness, and uses checked math. The result includes the immediate
   `effectiveFeeX24`; `pNext` equals the cached operator anchor.

The canonical example does not subscribe to `newFlashblocks` or `pendingLogs`.
It intentionally gives up sub-block state and pending-log latency so it cannot
construct hybrid or rollback-prone snapshots.

## Data needed for a fully off-chain quote

The math crate does not call the chain. Your service must keep these values
warm in cache:

| Value | Source | Why it is needed |
| ----- | ------ | ---------------- |
| `anchorPrice`, `feeAskX24`, `feeBidX24`, `latestUpdateBlock` | block-pinned `state()` | Anchor, current directional fees, and freshness origin |
| `reserveX`, `reserveY` | block-pinned `getXReserve()` / `getYReserve()` | Active reserves from the same block as every other field |
| `maxPunishmentX24` | block-pinned `maxPunishmentX24()` | Maximum immediate directional fee increment |
| `blockDelay` | block-pinned `blockDelay()` | Fail-closed operator-update freshness check |
| `paused` | block-pinned `paused()` | Do not quote executable swaps while paused |
| `isWhitelisted(QUOTE_CALLER_ADDRESS)` | block-pinned `isWhitelisted()` | Decides whether multiplier is `1` |
| `blacklistFeeMultiplier` | block-pinned `blacklistFeeMultiplier()` | Multiplier for non-whitelisted callers |
| `snapshotBlock` | confirmed `newHeads.number`, verified by HTTP `eth_getBlockByHash` | Numeric identity and freshness height of the snapshot |
| `snapshotBlockHash` | confirmed `newHeads.hash`, verified by HTTP `eth_getBlockByHash` | EIP-1898 block hash passed to every view above with `requireCanonical=true` |

The HTTP endpoint must support the EIP-1898 block selector object
`{"blockHash": "0x…", "requireCanonical": true}` for `eth_call`. If it does
not, or if the observed hash is missing, non-canonical, unavailable, or resolves
to a different number, refresh fails closed and Redis remains unhealthy.

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
`QUOTE_INTERVAL_SECS`, `SNAPSHOT_TIMEOUT_SECS`,
`REDIS_CONNECT_TIMEOUT_SECS`, `RUST_LOG`. `SEED_TIMEOUT_SECS` remains a legacy
fallback for `SNAPSHOT_TIMEOUT_SECS`.

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

| Key | Type | TTL | Content |
| --- | --- | --- | --- |
| `pmm:canonicalSnapshot:<pool>:<caller>` | JSON | — | Complete block-pinned pool and caller policy snapshot |
| `pmm:synchronized:<pool>:<caller>` | string | 30 s | `1` only after atomic publication; `0` during refresh/disconnect |
| `pmm:snapshotGeneration:<pool>:<caller>` | integer | — | Monotonic invalidation token guarding snapshot publication |
| `pmm:connectionEpochCounter:<pool>:<caller>` | integer | — | Monotonic WS connection epoch allocator |
| `pmm:activeConnectionEpoch:<pool>:<caller>` | integer | — | Epoch allowed to start and atomically publish snapshots |

Inspect:

```sh
docker exec lunarbase-redis redis-cli MONITOR
docker exec lunarbase-redis redis-cli KEYS '*'
```

## Scope

The example is **read-only**: no transaction signing, no swap calldata
construction, no anchor-price computation, no CEX integration.

The snapshot payload is persistent, but it is unusable without the expiring
synchronized lease. If heads stop, the WebSocket disconnects, or a refresh
fails, the quoter fails closed instead of presenting old state as fresh.
Every payload records both `snapshotBlock` and `snapshotBlockHash`.

Pending logs and individual contract events never mutate Redis. This loses
sub-block visibility by design: after an unconfirmed swap or operator update,
the example continues to serve only its last healthy confirmed snapshot until
the next confirmed-head refresh completes.

The example uses separate Redis connections for snapshot publication and quote
reads. Keep that separation in production so slow client requests cannot block
head processing. Run exactly one snapshot writer per pool/caller namespace.
For multiple execution callers, use one caller-scoped namespace each.

See [`math/rust/lunarbase-pmm-math`](../../math/rust/lunarbase-pmm-math) for
the quoter API.
