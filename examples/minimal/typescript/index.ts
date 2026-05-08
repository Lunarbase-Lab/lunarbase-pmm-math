// Minimal example pulling @lunarbase-lab/pmm-math directly from npm.
//
// One-shot from this directory:
//   npm install
//   npm run run
//
// `npm install` resolves the right native binary for your OS/arch via
// optionalDependencies — no build step, no Rust toolchain required.
import { quoteXToY, quoteYToX, type QuoteParams } from "@lunarbase-lab/pmm-math";

const Q48 = (1n << 48n).toString();

// Q24 = 2^24 represents 100% in the directional fee fields.
const Q24 = Number(1 << 24);

const baseParams: Omit<QuoteParams, "amountIn"> = {
  sqrtPriceX48: Q48,
  anchorSqrtPriceX48: Q48,
  // 0.10% fees on both sides.
  feeAskX24: Math.floor(Q24 / 1000),
  feeBidX24: Math.floor(Q24 / 1000),
  reserveX: "1000000000",
  reserveY: "1000000000",
  // Concentration K is Q20.12. Legacy plain-int K=5000 maps to 5000 << 12.
  concentrationKQ12: 5000 << 12,
};

const xToY = quoteXToY({ ...baseParams, amountIn: "10000" });
console.log(`X->Y  in=10000  out=${xToY.amountOut}  fee=${xToY.fee}  pNext=${xToY.sqrtPriceNext}`);

const yToX = quoteYToX({ ...baseParams, amountIn: "10000" });
console.log(`Y->X  in=10000  out=${yToX.amountOut}  fee=${yToX.fee}  pNext=${yToX.sqrtPriceNext}`);
