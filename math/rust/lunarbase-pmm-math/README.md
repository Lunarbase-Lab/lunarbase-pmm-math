# lunarbase-pmm-math

[![crates.io](https://img.shields.io/crates/v/lunarbase-pmm-math.svg)](https://crates.io/crates/lunarbase-pmm-math)
[![docs.rs](https://docs.rs/lunarbase-pmm-math/badge.svg)](https://docs.rs/lunarbase-pmm-math)

Pure Rust port of the on-chain LunarBase Curve PMM quoting math (single-price
**Q32.48** design). Bit-for-bit identical with the Solidity reference,
validated by deterministic and fuzz vectors generated from the contract.

- No `unsafe`, no FFI, no allocations on the hot path.
- Single dependency: [`ruint`](https://crates.io/crates/ruint) for 256-bit ints.
- Sqrt-prices are uint80, exposed as `u128`; products that exceed 128 bits
  widen to `U256` internally.

## Quick start

```toml
[dependencies]
lunarbase-pmm-math = "0.2"
```

```rust
use lunarbase_pmm_math::{plain_to_q12_concentration_k, quote_x_to_y, PoolParams, U256};

let params = PoolParams {
    sqrt_price_x48: 1u128 << 48,            // price = 1.0
    fee_ask_x24: 0,                         // Q24, charged on Y→X
    fee_bid_x24: (1u32 << 24) / 100,        // 1%, charged on X→Y
    reserve_x: 1_000_000_000_000_000_000,
    reserve_y: 1_000_000_000_000_000_000,
    concentration_k: plain_to_q12_concentration_k(5000),
};

let r = quote_x_to_y(&params, U256::from(1_000_000_000_000_000_000u128));
let _ = (r.amount_out, r.sqrt_price_next, r.fee);
```

## API surface

| Item                                                                      | Purpose                                              |
| ------------------------------------------------------------------------- | ---------------------------------------------------- |
| `PoolParams`                                                              | Snapshot fed to a quote.                             |
| `QuoteResult { amount_out, sqrt_price_next, fee }`                        | `amount_out` is **net** of `fee`.                    |
| `quote_x_to_y(params, dx)` / `quote_y_to_x(params, dy)`                   | Bit-exact mirrors of Solidity `SwapLib`.             |
| `price_to_sqrt_price_x48(price)` / `sqrt_price_x48_to_price(p)`           | `f64` decimal price ↔ Q32.48 sqrt-price.             |
| `plain_to_q12_concentration_k(k)` / `q12_to_plain_concentration_k(k_q12)` | Plain `K` ↔ Q20.12 `concentration_k`.                |

Legacy Q64.96 helpers (`sqrt_price_x48_to_x96`, `sqrt_price_x96_to_x48`,
`price_to_sqrt_price_x96`, `sqrt_price_x96_to_price`) are retained as
`#[deprecated]` one-shot migration aids for pre-Q48 serialised state.

## Testing

```sh
cargo test --release   # unit + 21 deterministic + ~8k fuzz vectors
```

Vectors are regenerated from the on-chain Foundry suite. Any divergence with
the contract fails CI before publish.

## Companion N-API binding

Same math, published for Node.js as
[`@lunarbase-lab/pmm-math`](https://www.npmjs.com/package/@lunarbase-lab/pmm-math).
Both crates share the upstream test vectors.

## License

Dual-licensed under MIT or Apache-2.0.
