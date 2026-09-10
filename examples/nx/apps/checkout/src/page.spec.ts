import { renderCheckout } from './page';

test('renders checkout', () => {
  expect(renderCheckout([{ sku: 'a', qty: 1 }])).toContain('[pay]');
});
