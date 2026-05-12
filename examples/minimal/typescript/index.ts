// Minimal example pulling @lunarbase-lab/pmm-math directly from npm.
//
// One-shot from this directory:
//   npm install
//   npm run run
//
// `npm install` resolves the right native binary for your OS/arch via
// optionalDependencies — no build step, no Rust toolchain required.
//
// NOTE: this example targets the fix/incident release line (single-price
// Q32.48 design). After @lunarbase-lab/pmm-math is republished from this
// branch the QuoteParams type will expose `sqrtPriceX48` and the example
// will type-check cleanly. Until then bump the dep version locally.
import { quoteXToY, quoteYToX } from "@lunarbase-lab/pmm-math";

// Q32.48 = 2^48 represents price = 1.0 in the sqrt-price encoding.
const Q48 = (1n << 48n).toString();

// Q24 = 2^24 represents 100% in the directional fee fields.
const Q24 = Number(1 << 24);

const baseParams = {
  sqrtPriceX48: Q48,
  // 0.10% fees on both sides.
  feeAskX24: Math.floor(Q24 / 1000),
  feeBidX24: Math.floor(Q24 / 1000),
  reserveX: "1000000000",
  reserveY: "1000000000",
  // Concentration K is Q20.12. Legacy plain-int K=5000 maps to 5000 << 12.
  concentrationK: 5000 << 12,
} as const;

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const xToY = quoteXToY({ ...baseParams, amountIn: "10000" } as any);
console.log(`X->Y  in=10000  out=${xToY.amountOut}  fee=${xToY.fee}  pNext=${xToY.sqrtPriceNext}`);

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const yToX = quoteYToX({ ...baseParams, amountIn: "10000" } as any);
console.log(`Y->X  in=10000  out=${yToX.amountOut}  fee=${yToX.fee}  pNext=${yToX.sqrtPriceNext}`);
