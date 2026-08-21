# @lunarbase-lab/pmm-math

N-API binding exposing [`lunarbase-pmm-math`](https://crates.io/crates/lunarbase-pmm-math)
to Node.js. Bit-exact mirror of the on-chain LunarBase v1 quote path with
v2-style linear size slippage, verified against deterministic and fuzz vectors
generated from the current Solidity contract.

## Install

```bash
npm install @lunarbase-lab/pmm-math
```

The matching `.node` binary is pulled via `optionalDependencies`. Supported
platforms:

| Sub-package                                | OS / Arch          |
| ------------------------------------------ | ------------------ |
| `@lunarbase-lab/pmm-math-darwin-arm64`     | macOS arm64        |
| `@lunarbase-lab/pmm-math-linux-x64-gnu`    | Linux x64 (glibc)  |
| `@lunarbase-lab/pmm-math-linux-arm64-gnu`  | Linux arm64 (glibc)|
| `@lunarbase-lab/pmm-math-linux-x64-musl`   | Linux x64 (musl / Alpine) |

Open an issue if you need Linux arm64 musl, darwin-x64, or win32-x64.

## Usage

```ts
import {
  quoteXToY,
  plainToQ12ConcentrationK,
  priceToSqrtPriceX96,
  type QuoteParams,
} from "@lunarbase-lab/pmm-math";

const params: QuoteParams = {
  // Q64.96 sqrt-price (uint160). 2^96 = price 1.0; use priceToSqrtPriceX96
  // for arbitrary decimal prices.
  sqrtPriceX96: priceToSqrtPriceX96(1.0),
  feeAskX24: 0,           // Q24, charged on Y→X
  feeBidX24: 838860,      // Q24, ≈ 5% charged on X→Y
  reserveX: "1000000000000000000000",
  reserveY: "1000000000000000000000",
  // Linear slippage K uses the legacy Q20.12 wire/storage encoding.
  concentrationK: plainToQ12ConcentrationK(5000),
  amountIn: "1000000000000000000",
};

const r = quoteXToY(params);
console.log(r.amountOut, r.sqrtPriceNext, r.fee);
```

All big-integer fields cross the JS ↔ native boundary as **strings** (decimal
or `0x`-hex). Output amounts are decimal strings.

`1_000_000` protocol BPS represents 100%. The computed size slippage is linear
in swap cash value and capped at `100_000` (10%). Directional Q24 fees remain a
separate v1 component and are applied after slippage.

### API surface

| Function                                                          | Purpose                                                |
| ----------------------------------------------------------------- | ------------------------------------------------------ |
| `quoteXToY(params)` / `quoteYToX(params)`                         | Bit-exact mirrors of Solidity `SwapLib`.               |
| `priceToSqrtPriceX96(price)` / `sqrtPriceX96ToPrice(p)`           | `number` price ↔ Q64.96 sqrt-price.                    |
| `price_to_sqrt_price_x96(price)` / `sqrt_price_x96_to_price(p)`   | Compatibility aliases for the X96 converter helpers.   |
| `plainToQ12ConcentrationK(k)` / `q12ToPlainConcentrationK(kQ12)`  | Plain `K` ↔ Q20.12 `concentrationK`.                   |

## Pure-Rust crate

The same math is also published as
[`lunarbase-pmm-math`](https://crates.io/crates/lunarbase-pmm-math) on
crates.io.

## License

Dual-licensed under MIT or Apache-2.0.
