import { quoteXToY, quoteYToX } from '@dark-pools/lunarbase-math'

const r = quoteXToY({
  sqrtPriceX48: '12781077694964135',
  feeAskX24: 0,
  feeBidX24: 50_000, // ~0.298% in Q24
  reserveX: '100000000000000000000',
  reserveY: '196452000000000000000000',
  concentrationKQ12: 5000,
  amountIn: '1000000'
})
console.log(r);
