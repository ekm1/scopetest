import { renderApp } from './app';

test('renders the app', () => {
  expect(renderApp()).toContain('[buy]');
});
