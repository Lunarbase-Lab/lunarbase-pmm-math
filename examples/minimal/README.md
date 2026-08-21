# Minimal examples

Smallest end-to-end usage of `lunarbase-pmm-math` from each supported
language.

Paths are relative to the repository root.

## Rust

```sh
cargo run --manifest-path examples/minimal/rust/Cargo.toml
```

The example pins the `0.3.0` API, uses Q64.96 state, and passes
`fee_multiplier = 1` explicitly for the whitelisted aggregator path:

```
X->Y  in=10000  out=9990  fee=9  pNext=79228120012965991766737027072
Y->X  in=10000  out=9990  fee=9  pNext=79228200293196200550705659904
```

`pNext` is the informational size-adjusted execution price returned by the quote; current
Pool contracts do not persist it as their next anchor.

The example crate has its own `Cargo.toml` and is excluded from the workspace.

## Go

```sh
go run ./examples/minimal/go
```

`examples/minimal/go/go.mod` uses a `replace` directive for the local
`math/go` package. Drop it and pin a tagged version to depend on the
published module instead.

## TypeScript / Node.js

```sh
cd examples/minimal/typescript
npm install
npm run run
```

The `prerun` script builds the napi addon via `@napi-rs/cli`. The first
invocation takes ~30 s while cargo compiles `napi-derive`; subsequent runs
are instant.

Requirements: Node.js 18+ on Linux or macOS.
