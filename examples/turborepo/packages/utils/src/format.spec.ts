import { currency } from './format';

test('formats cents', () => {
  expect(currency(1250)).toBe('$12.50');
});
