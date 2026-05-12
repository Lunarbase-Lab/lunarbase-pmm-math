# Minimal examples

Smallest end-to-end usage of `lunarbase-pmm-math` from each supported
language. All three produce the same two-line output:

```
X->Y  in=10000  out=9974  fee=9  pNext=281474976710321
Y->X  in=10000  out=9974  fee=9  pNext=281474976710991
```

Paths are relative to the repository root.

## Rust

```sh
cargo run --manifest-path examples/minimal/rust/Cargo.toml
```

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
