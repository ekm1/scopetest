import { total } from './cart';

test('totals cart lines', () => {
  expect(total([{ sku: 'a', qty: 2 }])).toBe(2);
});
