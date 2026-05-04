# lunarbase-pmm-math

[![CI](https://github.com/lunarbase/lunarbase-pmm-math/actions/workflows/ci.yml/badge.svg)](https://github.com/lunarbase/lunarbase-pmm-math/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)

Reference implementations of the LunarBase Curve PMM quoting math, kept
bit-for-bit identical with the on-chain Solidity contract.

## Layout

| Path                                      | Crate / module                     | Purpose                                                                                         |
| ----------------------------------------- | ---------------------------------- | ----------------------------------------------------------------------------------------------- |
| `math/rust/lunarbase-pmm-math/`           | `lunarbase-pmm-math` (rlib)        | Pure Rust core. Portable, no `unsafe`, no FFI.                                                  |
| `math/rust-node/lunarbase-pmm-math-node/` | `lunarbase-pmm-math-node` (cdylib) | N-API binding for Node.js. Native, OS+arch specific.                                            |
| `math/go/`                                | `lunarbasepmm`                     | Pure Go mirror of the Rust public API.                                                          |
| `examples/minimal/{rust,typescript,go}/`  | —                                  | Minimal end-to-end usage samples. See [examples/minimal/README.md](examples/minimal/README.md). |

The Rust core and the Go mirror are validated against the same JSONL test
vectors generated from the on-chain reference (`deterministic_vectors.jsonl`,
`fuzz_vectors.jsonl`).

## Public API

All three implementations expose the same surface:

```
PoolParams { sqrt_price_x48, anchor_sqrt_price_x48, fee_q48,
             reserve_x, reserve_y, concentration_k }

QuoteResult { amount_out, sqrt_price_next, fee }

quote_x_to_y(params, dx) -> QuoteResult
quote_y_to_x(params, dy) -> QuoteResult
```

Names follow each language's conventions (`QuoteXToY` in Go,
`quote_x_to_y` in Rust, `quoteXToY` in the N-API binding).

## Requirements

- Rust 1.75+ (stable) with `cargo`
- Go 1.22+
- For cross-compilation: `zig` and `cargo-zigbuild` (see _Cross-compilation_)
- For the Node.js binding: Node.js 18+ to load the produced `.node` file

Install all cross-compilation tooling in one command:

```sh
make setup-cross
```

This installs `zig` (via Homebrew on macOS), `cargo-zigbuild`, and adds the
common `rustup` targets.

## Build & test

The top-level `Makefile` is the single entry point.

```sh
make            # build + test all three packages for the host
make ci         # fmt-check + lint + test (matches CI)
make build      # build only
make test       # test only
make clean      # clean all build artifacts
make fmt        # format Rust + Go sources
make lint       # cargo clippy + go vet
```

Per-package targets are also available: `rust-build`, `rust-test`,
`node-build`, `node-test`, `go-build`, `go-test`, etc. Run `make -n <target>`
to inspect the underlying command.

## Cross-compilation

Cross-compilation uses [`cargo-zigbuild`][zigbuild], which links through
`zig cc`. **No Docker or VM is required**; everything runs on the host.

[zigbuild]: https://github.com/rust-cross/cargo-zigbuild

### Rust core

```sh
make rust-cross TARGET=aarch64-unknown-linux-gnu
make rust-cross TARGET=x86_64-unknown-linux-gnu.2.17    # pin minimum glibc
make rust-cross TARGET=x86_64-unknown-linux-musl        # static, for Alpine
```

### N-API binding

```sh
make node-cross-linux-x64
make node-cross-linux-arm64
make node-cross-linux-musl
make node-cross-mac-x64
make node-cross-mac-arm64
make node-cross-mac-universal     # fat binary (x86_64 + arm64)
make node-cross-all               # everything above, in one command
```

Output paths follow `target/<triple>/release/liblunarbase_pmm_math_node.{so,dylib}`.
Rename the resulting library to `.node` before loading from Node.js, or use
[`@napi-rs/cli`][napi-cli] to publish per-platform npm packages.

[napi-cli]: https://napi.rs/docs/cli/build

Windows targets are not currently supported by `cargo-zigbuild`; build the
addon on a Windows runner (e.g. GitHub Actions `windows-latest`) instead.

### Go package

The Go mirror has no native dependencies and cross-compiles with the standard
toolchain — `zig` is not needed.

```sh
make go-cross-linux-amd64
make go-cross-linux-arm64
make go-cross-darwin-amd64
make go-cross-darwin-arm64
make go-cross-windows-amd64
make go-cross-all
```

## Platform notes

| Package                   | OS-/arch-dependent? | Notes                                                                    |
| ------------------------- | ------------------- | ------------------------------------------------------------------------ |
| `rust/lunarbase-pmm-math` | No                  | Pure Rust on `ruint`. Identical artifact across platforms.               |
| `go/`                     | No                  | Pure Go on `holiman/uint256`. CGO not used.                              |
| `rust-node/...`           | **Yes**             | Produces `.so`/`.dylib` per (os, arch). Standard for Node native addons. |

## Test vectors

The same JSONL vectors live in:

- `math/rust/lunarbase-pmm-math/deterministic_vectors.jsonl`
- `math/rust/lunarbase-pmm-math/fuzz_vectors.jsonl`
- `math/go/testdata/deterministic_vectors.jsonl`
- `math/go/testdata/fuzz_vectors.jsonl`

Both implementations replay every vector and assert bit-exact equality with
the on-chain reference. When regenerating vectors from Solidity, update both
locations.

## License

Dual-licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Unless you explicitly state otherwise, any contribution
intentionally submitted for inclusion in this work shall be dual-licensed as
above, without any additional terms or conditions.

## Adding a Rust target

Any triple supported by `rustc` and `zig` works. Add it to your toolchain
once:

```sh
rustup target add <triple>
make rust-cross TARGET=<triple>
```

To make it a first-class `make` target, append a recipe to the relevant
`*-cross-*` block in the `Makefile`.
