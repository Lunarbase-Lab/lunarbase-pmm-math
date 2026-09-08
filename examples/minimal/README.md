# Minimal examples

Smallest end-to-end usage of `lunarbase-pmm-math` from each supported
language.

Paths are relative to the repository root.

## Rust

```sh
cargo run --manifest-path examples/minimal/rust/Cargo.toml
```

The Rust example pins the `0.4.1` API, uses the full Q64.96 `uint160` anchor
domain, and passes `fee_multiplier = 1` for the whitelisted aggregator path.
It prints both the immediate-punishment quote and the committed fee transition.

```
X->Y  in=10000  out=9990  fee=10  effectiveFee=16786  pNext=79228162514264337593543950336
      desiredPunishment=9 appliedPunishment=9 nextBidFee=16786
Y->X  in=10000  out=9990  fee=10  effectiveFee=16786  pNext=79228162514264337593543950336
```

`pNext` is retained for contract ABI compatibility and equals the operator
anchor. Punishment is included in the triggering quote's effective directional
fee and is persisted for the next swap only after successful settlement.

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

The package downloads the native addon for the current platform through npm
`optionalDependencies`; consumers do not need a Rust toolchain.

Requirements: Node.js 18+ on macOS arm64, Linux x64 glibc (GLIBC 2.17+),
Linux arm64 glibc, or Linux x64 musl/Alpine.
