# @lunarbase-lab/pmm-math

N-API binding exposing [`lunarbase-pmm-math`](https://crates.io/crates/lunarbase-pmm-math)
to Node.js. Bit-exact mirror of the on-chain LunarBase Curve PMM quoting
math (`fix/incident` branch, single-price **Q32.48** design); every quote is
verified against deterministic and fuzz vectors generated from the on-chain
Solidity contract.

## Install

```bash
npm install @lunarbase-lab/pmm-math
```

The native `.node` binary is shipped per-platform via npm's
`optionalDependencies` mechanism — only the binary matching your OS / arch
is downloaded.

### Supported platforms

| OS    | Architecture  | Sub-package                                |
| ----- | ------------- | ------------------------------------------ |
| macOS | arm64         | `@lunarbase-lab/pmm-math-darwin-arm64`     |
| Linux | x64 (glibc)   | `@lunarbase-lab/pmm-math-linux-x64-gnu`    |
| Linux | arm64 (glibc) | `@lunarbase-lab/pmm-math-linux-arm64-gnu`  |

Other targets (`linux-x64-musl`, `darwin-x64`, `win32-x64-msvc`) are not
currently shipped — open an issue if you need one.

## Usage

```ts
import {
  quoteXToY,
  quoteYToX,
  plainToQ12ConcentrationK,
  priceToSqrtPriceX48,
  type QuoteParams,
  type QuoteResult,
} from "@lunarbase-lab/pmm-math";

const params: QuoteParams = {
  // Q32.48 sqrt-price (uint80). Single canonical price — operator-set,
  // swaps do not move it. Pass a decimal or 0x-hex string.
  //
  // 281_474_976_710_656 = 2^48 == price 1.0. For a price of 2500 (e.g.
  // ETH/USDC raw units), use `priceToSqrtPriceX48(2500)` instead.
  sqrtPriceX48: "281474976710656",
  // Directional fees (uint24, Q24, where 2^24 represents 100%).
  feeAskX24: 0,      // charged on yToX swaps
  feeBidX24: 838860, // charged on xToY swaps (≈ 5%)
  // Reserves (uint112).
  reserveX: "1000000000000000000000",
  reserveY: "1000000000000000000000",
  // Concentration multiplier (uint32, Q20.12).
  // Effective K = concentrationK / 2^12. For a plain `K=5000`, pass
  // `plainToQ12ConcentrationK(5000)` (== 20_480_000).
  concentrationK: plainToQ12ConcentrationK(5000),
  // Input amount in the source token (decimal or 0x-hex).
  amountIn: "1000000000000000000",
};

const result: QuoteResult = quoteXToY(params);
console.log(result.amountOut, result.sqrtPriceNext, result.fee);
// → "949987816809994001" "281474976660325" "49999308586734514"
```

All big-integer fields (`sqrtPriceX48`, `reserveX`, `reserveY`, `amountIn`)
are passed as **strings** — decimal or `0x`-prefixed hex — to preserve full
precision across the JS ↔ native boundary. Outputs (`amountOut`,
`sqrtPriceNext`, `fee`) are decimal strings.

### API surface

| Function                                                | Purpose                                                                 |
| ------------------------------------------------------- | ----------------------------------------------------------------------- |
| `quoteXToY(params)`                                     | Quote a token-X-in / token-Y-out swap. Bit-exact mirror of `SwapLib._quoteXToY`. |
| `quoteYToX(params)`                                     | Quote a token-Y-in / token-X-out swap. Bit-exact mirror of `SwapLib._quoteYToX`. |
| `priceToSqrtPriceX48(price)` / `sqrtPriceX48ToPrice(p)` | Decimal `f64` price ↔ Q32.48 sqrt-price (uint80 as decimal string).     |
| `plainToQ12ConcentrationK(k)` / `q12ToPlainConcentrationK(kQ12)` | Plain integer `K` ↔ Q20.12 `concentrationK`.                            |

### Legacy Q64.96 migration helpers

Retained for callers still carrying pre-Q48 serialised state. Marked
deprecated in `index.d.ts`:

| Function                                | Purpose                                                          |
| --------------------------------------- | ---------------------------------------------------------------- |
| `sqrtPriceX48ToX96(pX48)`               | Lift Q32.48 → Q64.96 by `<<48`. Lossless.                        |
| `sqrtPriceX96ToX48(pX96)`               | Lower Q64.96 → Q32.48 by `>>48`. Truncates 48 fractional bits.   |
| `priceToSqrtPriceX96(price)`            | Decimal price → Q64.96 sqrt-price.                               |
| `sqrtPriceX96ToPrice(pX96)`             | Q64.96 sqrt-price → decimal price.                               |

### tsconfig

For the ESM imports above to type-check you need
`"moduleResolution": "node16"` or `"bundler"`. A minimal setup:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ES2022",
    "moduleResolution": "node16",
    "esModuleInterop": true,
    "strict": true
  }
}
```

## Pure-Rust crate

The same math is also published on crates.io as
[`lunarbase-pmm-math`](https://crates.io/crates/lunarbase-pmm-math). Both
packages share the upstream test vectors and are guaranteed bit-identical
with the on-chain reference.

## Building from source

See the [maintainer notes](https://github.com/Lunarbase-Lab/lunarbase-pmm-math/tree/main/math/rust-node/lunarbase-pmm-math-node)
in the source repository for cross-compilation, musl/Alpine notes, and
release process.

## License

Dual-licensed under MIT or Apache-2.0.
