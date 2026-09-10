import { Card } from './card';

test('renders a title', () => {
  expect(Card('hi')).toBe('<card>hi</card>');
});
