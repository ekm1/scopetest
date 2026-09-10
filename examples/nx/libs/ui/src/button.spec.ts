import { Button } from './button';

test('renders a label', () => {
  expect(Button('go')).toBe('[go]');
});
