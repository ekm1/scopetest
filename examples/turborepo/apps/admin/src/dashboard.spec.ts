import { renderTotal } from './dashboard';

test('renders a total', () => {
  expect(renderTotal(500)).toBe('Total: $5.00');
});
