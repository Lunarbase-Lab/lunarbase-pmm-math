import { quoteXToY, quoteYToX } from '@lunarbase-lab/pmm-math'

const r = quoteXToY({
  // Q64.96 sqrt-price (uint160). 3_543_191_142_285_914_096_597_660_073_984 ≈ sqrt(2000) * 2^96
  // (ETH/USDC-style pair, raw units).
  sqrtPriceX96: '3543191142285914096597660073984',
  feeAskX24: 0,
  feeBidX24: 50_000, // ~0.298% in Q24
  reserveX: '100000000000000000000',
  reserveY: '196452000000000000000000',
  concentrationK: 5000,
  amountIn: '1000000'
})
console.log(r);
