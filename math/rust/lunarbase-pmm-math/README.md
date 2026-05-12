# lunarbase-pmm-math

[![crates.io](https://img.shields.io/crates/v/lunarbase-pmm-math.svg)](https://crates.io/crates/lunarbase-pmm-math)
[![docs.rs](https://docs.rs/lunarbase-pmm-math/badge.svg)](https://docs.rs/lunarbase-pmm-math)

Pure Rust port of the on-chain LunarBase Curve PMM quoting math
(`fix/incident` branch, single-price **Q32.48** design). Bit-for-bit
identical with the Solidity reference, validated by deterministic and
fuzz vectors generated from the contract.

- No `unsafe`, no FFI, no allocations on the hot path.
- Single dependency: [`ruint`](https://crates.io/crates/ruint) for 256-bit
  fixed-width integers.
- Sqrt-prices are uint80, exposed as `u128` for ergonomics; intermediate
  products that exceed 128 bits widen to `U256` inside the math.

## Quick start

```toml
[dependencies]
lunarbase-pmm-math = "0.2"
```

```rust
use lunarbase_pmm_math::{
    plain_to_q12_concentration_k, quote_x_to_y, PoolParams, U256,
};

let params = PoolParams {
    // Q32.48 sqrt-price (uint80) — `1 << 48` represents price 1.0.
    sqrt_price_x48: 1u128 << 48,
    // Directional fees (Q24, where 2^24 represents 100%).
    fee_ask_x24: 0,                  // charged on yToX swaps
    fee_bid_x24: (1u32 << 24) / 100, // 1% charged on xToY swaps
    // Reserves (uint112).
    reserve_x: 1_000_000_000_000_000_000,
    reserve_y: 1_000_000_000_000_000_000,
    // Concentration multiplier in Q20.12. Plain `K=5000` is encoded as
    // `5000 << 12 == 20_480_000`.
    concentration_k: plain_to_q12_concentration_k(5000),
};

let result = quote_x_to_y(&params, U256::from(1_000_000_000_000_000_000u128));
let _ = result.amount_out;    // U256 net of fee
let _ = result.sqrt_price_next; // u128 (Q32.48), informational
let _ = result.fee;            // U256, in the output token
```

## API surface

| Item                                                    | Purpose                                                                 |
| ------------------------------------------------------- | ----------------------------------------------------------------------- |
| `PoolParams { sqrt_price_x48, fee_*_x24, reserve_*, concentration_k }` | Snapshot fed to a quote.                                  |
| `QuoteResult { amount_out, sqrt_price_next, fee }`      | Output of a quote; `amount_out` is **net** of `fee`.                    |
| `quote_x_to_y(params, dx) -> QuoteResult`               | Bit-exact mirror of Solidity `SwapLib._quoteXToY`.                      |
| `quote_y_to_x(params, dy) -> QuoteResult`               | Bit-exact mirror of Solidity `SwapLib._quoteYToX`.                      |
| `price_to_sqrt_price_x48(price)` / `sqrt_price_x48_to_price(p)` | `f64` decimal price ↔ Q32.48 sqrt-price.                         |
| `plain_to_q12_concentration_k(k)` / `q12_to_plain_concentration_k(k_q12)` | Plain integer `K` ↔ Q20.12 `concentration_k`.          |

### Legacy Q64.96 migration helpers

Retained for callers still carrying pre-Q48 serialised state. Marked
`#[deprecated]` — use only as one-shot conversion aids:

| Function                            | Purpose                                                          |
| ----------------------------------- | ---------------------------------------------------------------- |
| `sqrt_price_x48_to_x96(p_x48)`      | Lift Q32.48 → Q64.96 by `<<48`. Lossless.                        |
| `sqrt_price_x96_to_x48(p_x96)`      | Lower Q64.96 → Q32.48 by `>>48`. Truncates 48 fractional bits.   |
| `price_to_sqrt_price_x96(price)`    | Decimal price → Q64.96 sqrt-price (saturates at `U256::MAX`).    |
| `sqrt_price_x96_to_price(p_x96)`    | Q64.96 sqrt-price → decimal price (`(p / 2^96)^2`).              |

## Design notes

- **Single canonical sqrt-price.** There is no live-vs-anchor split. The
  operator-published Q32.48 sqrt-price *is* the only price; swaps compute a
  hypothetical `sqrt_price_next` but never write back into state. This
  removes the drift-based round-trip exploit class by construction.
- **Asymmetric directional fees.** `fee_bid_x24` charges X→Y; `fee_ask_x24`
  charges Y→X. Both are in Q24 (`2^24 == 100%`).
- **Concentration `c = K · r²` in Q48.** `K` is supplied as `Q20.12`; `r` is
  wealth-normalised by cascading `mulDiv(_, sqrtP, Q48)` twice to compute
  `reserveX * P` without precision loss.
- **`Lx` precision-fix.** Computed as
  `mulDiv(reserveX, sqrtP * pAsk, Q48 * (pAsk - sqrtP))`. Avoids the naive
  intermediate `priceProduct / Q96` truncation that would discard most of
  the sqrt-price precision at small `sqrtP`.

## Testing

The crate ships its own deterministic and fuzz vectors:

```sh
cargo test --release   # 16 unit + 21 deterministic + 7925 fuzz vectors
```

Vectors are regenerated from the on-chain Foundry suite
(`forge test --match-contract PoolTestVectors` /
`PoolFuzzVectors --fuzz-runs 10000`); any divergence with the contract
fails CI before publish.

## Companion N-API binding

The same math is also published as
[`@lunarbase-lab/pmm-math`](https://www.npmjs.com/package/@lunarbase-lab/pmm-math)
for Node.js. Both crates share the upstream test vectors and are guaranteed
byte-identical against the on-chain Solidity reference.

## License

Dual-licensed under MIT or Apache-2.0.
