// Minimal example: quote a swap in both directions and print the results.
//
// Run from this directory:
//   npm install
//   npm run run
//
// `prerun` builds the napi addon in math/rust-node/lunarbase-pmm-math-node,
// which is consumed here as a local file dependency.
import { quoteXToY, quoteYToX, type QuoteParams } from "lunarbase-pmm-math-node";

const Q48 = (1n << 48n).toString();

const baseParams: Omit<QuoteParams, "amountIn"> = {
  sqrtPriceX48: Q48,
  anchorSqrtPriceX48: Q48,
  feeQ48: (1n << 44n).toString(),
  reserveX: "1000000000",
  reserveY: "1000000000",
  concentrationK: 5_000,
};

const xToY = quoteXToY({ ...baseParams, amountIn: "10000" });
console.log(`X->Y  in=10000  out=${xToY.amountOut}  fee=${xToY.fee}  pNext=${xToY.sqrtPriceNext}`);

const yToX = quoteYToX({ ...baseParams, amountIn: "10000" });
console.log(`Y->X  in=10000  out=${yToX.amountOut}  fee=${yToX.fee}  pNext=${yToX.sqrtPriceNext}`);
