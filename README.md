# lunarbase-pmm-math

[![CI](https://github.com/Lunarbase-Lab/lunarbase-pmm-math/actions/workflows/ci.yml/badge.svg)](https://github.com/Lunarbase-Lab/lunarbase-pmm-math/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)

Reference implementations of the LunarBase Curve PMM quoting math —
bit-for-bit identical with the on-chain Solidity contract, validated by
shared JSONL test vectors.

## Layout

| Path                                      | Crate / module               | Purpose                                  |
| ----------------------------------------- | ---------------------------- | ---------------------------------------- |
| `math/rust/lunarbase-pmm-math/`           | `lunarbase-pmm-math`         | Pure Rust core. Portable, no `unsafe`.   |
| `math/rust-node/lunarbase-pmm-math-node/` | `lunarbase-pmm-math-node`    | N-API binding for Node.js (per OS/arch). |
| `math/go/`                                | `lunarbasepmm`               | Pure Go mirror of the same API.          |
| `examples/minimal/{rust,go,typescript}/`  | —                            | Smallest end-to-end usage per language.  |

## Public API

Q64.96 sqrt-price (`uint160`) design, mirroring the current on-chain Pool:

```
PoolParams {
    sqrt_price_x96,        // uint160, Q64.96 — operator-published sqrt-price
    fee_ask_x24,           // uint24,  Q24 — fee on Y→X
    fee_bid_x24,           // uint24,  Q24 — fee on X→Y
    reserve_x,             // uint112
    reserve_y,             // uint112
    concentration_k,       // uint32,  Q20.12 (effective K = stored / 2^12)
}

quote_x_to_y(params, dx) -> QuoteResult { amount_out, sqrt_price_next, fee }
quote_y_to_x(params, dy) -> QuoteResult
quote_x_to_y_with_multiplier(params, dx, fee_multiplier) -> QuoteResult
quote_y_to_x_with_multiplier(params, dy, fee_multiplier) -> QuoteResult
```

Names follow each language's conventions: `quote_x_to_y` (Rust), `QuoteXToY`
(Go), `quoteXToY` (N-API). Big numbers cross the N-API boundary as decimal or
`0x`-hex strings.

The plain quote helpers use `fee_multiplier = 1`, matching the Pool's
whitelisted/base-fee path. Public on-chain `quoteXToY`/`quoteYToX` derives the
multiplier from `msg.sender`; for non-whitelisted callers, pass
`pool.blacklistFeeMultiplier()` into the `*_with_multiplier` helpers (or the
N-API `feeMultiplier` field) to compare against the same on-chain quote path.

### Helpers

| Rust                                  | Go                               | N-API / TS                       | Purpose                                                              |
| ------------------------------------- | -------------------------------- | -------------------------------- | -------------------------------------------------------------------- |
| `price_to_sqrt_price_x96(price)`      | `PriceToSqrtPriceX96(price)`     | `priceToSqrtPriceX96(price)`     | `f64` decimal price → Q64.96.                                        |
| `sqrt_price_x96_to_price(p_x96)`      | `SqrtPriceX96ToPrice(pX96)`      | `sqrtPriceX96ToPrice(pX96)`      | Q64.96 → `f64` decimal price `(p/2^96)²`.                            |
| same as above                         | —                                | `price_to_sqrt_price_x96(price)` / `sqrt_price_x96_to_price(pX96)` | N-API compatibility aliases for the X96 converter helpers. |
| `plain_to_q12_concentration_k(k)`     | `PlainToQ12ConcentrationK(k)`    | `plainToQ12ConcentrationK(k)`    | Plain `K=100` → Q20.12 `409_600`.                                    |
| `q12_to_plain_concentration_k(k_q12)` | `Q12ToPlainConcentrationK(kQ12)` | `q12ToPlainConcentrationK(kQ12)` | Q20.12 → plain `K` (truncates).                                      |

## Build & test

```sh
make            # build + test all packages for the host
make ci         # fmt-check + lint + test (matches CI)
make bench      # micro-bench rust + go (not run in CI; noise-sensitive)
```

Per-package targets: `rust-test`, `go-test`, `node-test`, etc. Run
`make -n <target>` to inspect.

Requirements: Rust 1.75+, Go 1.22+, Node.js 18+ (for the binding only).

## Cross-compilation

Uses [`cargo-zigbuild`](https://github.com/rust-cross/cargo-zigbuild) — no
Docker. Install tooling once with `make setup-cross`, then:

```sh
make rust-cross TARGET=aarch64-unknown-linux-gnu
make node-cross-all           # all per-platform .node addons in one go
make go-cross-all             # standard Go cross-build, zig not needed
```

Windows targets are not supported by `cargo-zigbuild`; build the Node addon on
a Windows runner instead.

## Test vectors

Identical JSONL vectors live in `math/rust/lunarbase-pmm-math/{deterministic,fuzz}_vectors.jsonl`
and `math/go/testdata/`. Both implementations replay every vector and assert
bit-exact equality with the on-chain reference. Regenerate from the Foundry
suite; update both copies.

## Releases

| Registry  | Package                   | Install                            |
| --------- | ------------------------- | ---------------------------------- |
| crates.io | `lunarbase-pmm-math`      | `cargo add lunarbase-pmm-math`     |
| npm       | `@lunarbase-lab/pmm-math` | `npm install @lunarbase-lab/pmm-math` |

Cut by `.github/workflows/release.yml` on a `v*` tag push. To release:

```sh
# bump versions in:
#   - Cargo.toml [workspace.package].version
#   - math/rust-node/lunarbase-pmm-math-node/package.json (.version and all .optionalDependencies)
make publish-dry-run
git commit -am "release v0.X.Y"
git tag v0.X.Y && git push origin v0.X.Y
```

Required GitHub secrets: `CARGO_REGISTRY_TOKEN`, `NPM_TOKEN`
(npm Automation token with publish on the `@lunarbase-lab` scope).

## License

Dual-licensed under [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your
option.
