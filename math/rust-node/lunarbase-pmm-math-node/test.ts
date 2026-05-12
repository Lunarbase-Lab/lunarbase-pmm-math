import { quoteXToY, quoteYToX } from '@lunarbase-lab/pmm-math'

const r = quoteXToY({
  // Q32.48 sqrt-price (uint80). 12_587_943_637_803_939 ≈ sqrt(2000) * 2^48
  // (ETH/USDC-style pair, raw units).
  sqrtPriceX48: '12587943637803939',
  feeAskX24: 0,
  feeBidX24: 50_000, // ~0.298% in Q24
  reserveX: '100000000000000000000',
  reserveY: '196452000000000000000000',
  concentrationK: 5000,
  amountIn: '1000000'
})
console.log(r);
