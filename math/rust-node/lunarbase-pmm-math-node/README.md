# @lunarbase-lab/pmm-math

N-API binding exposing [`lunarbase-pmm-math`](https://crates.io/crates/lunarbase-pmm-math)
to Node.js. Bit-exact mirror of the on-chain LunarBase Curve PMM quoting math
(single-price **Q32.48** design), verified against deterministic and fuzz
vectors generated from the on-chain Solidity contract.

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

Open an issue if you need musl, darwin-x64, or win32-x64.

## Usage

```ts
import {
  quoteXToY,
  plainToQ12ConcentrationK,
  priceToSqrtPriceX48,
  type QuoteParams,
} from "@lunarbase-lab/pmm-math";

const params: QuoteParams = {
  // Q32.48 sqrt-price (uint80). 2^48 = price 1.0; use priceToSqrtPriceX48
  // for arbitrary decimal prices.
  sqrtPriceX48: priceToSqrtPriceX48(1.0),
  feeAskX24: 0,           // Q24, charged on Y→X
  feeBidX24: 838860,      // Q24, ≈ 5% charged on X→Y
  reserveX: "1000000000000000000000",
  reserveY: "1000000000000000000000",
  // Effective K = concentrationK / 2^12.
  concentrationK: plainToQ12ConcentrationK(5000),
  amountIn: "1000000000000000000",
};

const r = quoteXToY(params);
console.log(r.amountOut, r.sqrtPriceNext, r.fee);
```

All big-integer fields cross the JS ↔ native boundary as **strings** (decimal
or `0x`-hex). Output amounts are decimal strings.

### API surface

| Function                                                          | Purpose                                                |
| ----------------------------------------------------------------- | ------------------------------------------------------ |
| `quoteXToY(params)` / `quoteYToX(params)`                         | Bit-exact mirrors of Solidity `SwapLib`.               |
| `priceToSqrtPriceX48(price)` / `sqrtPriceX48ToPrice(p)`           | `number` price ↔ Q32.48 sqrt-price.                    |
| `plainToQ12ConcentrationK(k)` / `q12ToPlainConcentrationK(kQ12)`  | Plain `K` ↔ Q20.12 `concentrationK`.                   |

Legacy Q64.96 helpers (`sqrtPriceX48ToX96`, `sqrtPriceX96ToX48`,
`priceToSqrtPriceX96`, `sqrtPriceX96ToPrice`) are retained for migrating
pre-Q48 serialised state; marked deprecated in `index.d.ts`.

## Pure-Rust crate

The same math is also published as
[`lunarbase-pmm-math`](https://crates.io/crates/lunarbase-pmm-math) on
crates.io.

## License

Dual-licensed under MIT or Apache-2.0.
