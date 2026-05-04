# Minimal examples

The smallest possible end-to-end usage of `lunarbase-pmm-math` from each
supported language. Every example produces the same two lines of output —
quotes for `X→Y` and `Y→X` swaps on a symmetric pool — so you can sanity-check
that all three implementations agree:

```
X->Y  in=10000  out=9375  fee=624  pNext=281474887330570
Y->X  in=10000  out=9375  fee=624  pNext=281475066090770
```

All paths below are relative to the repository root.

## Rust

```sh
cargo run --manifest-path examples/minimal/rust/Cargo.toml
```

The example crate has its own `Cargo.toml` and is excluded from the workspace
so it doesn't slow down `cargo build` on the math crates.

## Go

```sh
go run ./examples/minimal/go
```

`examples/minimal/go/go.mod` uses a `replace` directive to consume the local
package in `math/go`. To depend on a published version instead, drop the
`replace` line and pin `github.com/lunarbase/lunarbase-pmm-math/math/go` to a
tagged version in the `require` block.

## TypeScript / Node.js

The TypeScript example consumes the `lunarbase-pmm-math-node` package via a
local `file:` dependency, so usage is just a clean ES-module import:

```ts
import { quoteXToY, quoteYToX } from "lunarbase-pmm-math-node";
```

Run it:

```sh
cd examples/minimal/typescript
npm install
npm run run
```

The `prerun` script builds the napi addon in
`math/rust-node/lunarbase-pmm-math-node` via `@napi-rs/cli`, which generates
the platform-specific `.node` binary plus an ESM/CJS dual-export wrapper and
TypeScript declarations. The first invocation takes ~30 s while cargo
compiles `napi-derive`; subsequent runs are instant.

Requirements: Node.js 18+ on Linux/macOS.
