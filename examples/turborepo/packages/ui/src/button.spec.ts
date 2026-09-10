import { Button } from './button';

test('renders a label', () => {
  expect(Button('ok')).toBe('[ok]');
});
