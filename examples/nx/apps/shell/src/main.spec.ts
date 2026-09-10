import { bootstrap } from './main';

test('boots the shell', () => {
  expect(bootstrap()).toContain('[menu]');
});
