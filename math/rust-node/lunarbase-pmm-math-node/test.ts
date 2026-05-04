import { quoteXToY, quoteYToX } from '@dark-pools/lunarbase-math'

const r = quoteXToY({
  sqrtPriceX48: '12781077694964135',
  feeQ48: '834787359210',
  reserveX: '100000000000000000000',
  reserveY: '196452000000000000000000',
  concentrationK: 5000,
  amountIn: '1000000'
})
console.log(r);
