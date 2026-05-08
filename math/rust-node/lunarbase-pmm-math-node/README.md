# @lunarbase-lab/pmm-math

N-API binding exposing [`lunarbase-pmm-math`](https://crates.io/crates/lunarbase-pmm-math)
to Node.js. Bit-exact mirror of the on-chain LunarBase Curve PMM quoting
math; every quote is verified against deterministic and fuzz vectors
generated from the on-chain Solidity contract.

## Install

```bash
npm install @lunarbase-lab/pmm-math
```

The native `.node` binary is shipped per-platform via npm's
`optionalDependencies` mechanism — only the binary matching your OS / arch
is downloaded.

### Supported platforms

| OS    | Architecture | Sub-package                                |
| ----- | ------------ | ------------------------------------------ |
| macOS | arm64        | `@lunarbase-lab/pmm-math-darwin-arm64`     |
| Linux | x64 (glibc)  | `@lunarbase-lab/pmm-math-linux-x64-gnu`    |
| Linux | arm64 (glibc)| `@lunarbase-lab/pmm-math-linux-arm64-gnu`  |

Other targets (`linux-x64-musl`, `darwin-x64`, `win32-x64-msvc`) are not
currently shipped — open an issue if you need one.

## Usage

```ts
import { quoteXToY, quoteYToX, type QuoteParams, type QuoteResult } from "@lunarbase-lab/pmm-math";

const params: QuoteParams = {
  // Live sqrt-price (uint80, Q32.48). Optional — defaults to anchorSqrtPriceX48.
  sqrtPriceX48: "281474976710656",
  // Operator-published anchor sqrt-price (uint80, Q32.48). Required.
  anchorSqrtPriceX48: "281474976710656",
  // Directional fees (uint24, Q24, where 2^24 represents 100%).
  feeAskX24: 0,        // charged on yToX swaps
  feeBidX24: 838860,   // charged on xToY swaps (≈ 5%)
  // Reserves (uint112).
  reserveX: "1000000000000000000000",
  reserveY: "1000000000000000000000",
  // Concentration multiplier (uint32) stored in Q20.12.
  // Effective K = concentrationK / 2^12. Legacy plain-int K=5000 maps to
  // 5000 << 12 = 20_480_000 here.
  concentrationK: 5000 << 12,
  // Input amount in the source token (decimal or 0x-hex).
  amountIn: "1000000000000000000",
  // Optional. Defaults to "1" (whitelisted swapper). Pass the on-chain
  // `blacklistFeeMultiplier` for the gated path.
  feeMultiplier: "1",
};

const result: QuoteResult = quoteXToY(params);
console.log(result.amountOut, result.sqrtPriceNext, result.fee);
```

All numeric inputs are passed as **strings** — decimal or `0x`-prefixed hex —
to preserve full `U256` precision. Outputs are decimal strings.

### tsconfig

For the imports above to type-check you need `"moduleResolution": "node16"`
or `"bundler"`. A minimal setup:

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
packages share the upstream test vectors and are guaranteed bit-identical.

## Building from source

See the [maintainer notes](https://github.com/Lunarbase-Lab/lunarbase-pmm-math/tree/main/math/rust-node/lunarbase-pmm-math-node)
in the source repository for cross-compilation, musl/Alpine notes, and
release process.

## License

Dual-licensed under MIT or Apache-2.0.
