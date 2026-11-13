'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const os = require('os');
const path = require('path');

const { createNodesV2 } = require('../src');
const executor = require('../src/executors/affected/executor');
const initGenerator = require('../src/generators/init/generator');
const { withLock } = require('../src/lib/scopetest');

function workspace(files) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'scopetest-nx-'));
  for (const [file, content] of Object.entries(files)) {
    fs.mkdirSync(path.dirname(path.join(root, file)), { recursive: true });
    fs.writeFileSync(path.join(root, file), content);
  }
  return root;
}

test('createNodesV2 infers a target per project with a test config', () => {
  const root = workspace({
    'libs/a/project.json': '{}',
    'libs/a/jest.config.ts': '',
    'libs/b/package.json': '{}',
    'libs/b/vite.config.ts': 'export default { test: { globals: true } }',
    'libs/c/project.json': '{}',
    'libs/c/vite.config.ts': 'export default { plugins: [] }',
    'tools/jest.config.js': '',
  });
  const [glob, createNodes] = createNodesV2;
  assert.match(glob, /jest,vitest,vite/);

  const results = createNodes(
    ['libs/a/jest.config.ts', 'libs/b/vite.config.ts', 'libs/c/vite.config.ts', 'tools/jest.config.js'],
    { targetName: 'scoped', threshold: 50 },
    { workspaceRoot: root }
  );

  assert.deepStrictEqual(results.map(([file]) => file), ['libs/a/jest.config.ts', 'libs/b/vite.config.ts']);
  const target = results[0][1].projects['libs/a'].targets.scoped;
  assert.strictEqual(target.executor, 'scopetest-nx:affected');
  assert.strictEqual(target.cache, false);
  assert.deepStrictEqual(target.options, { runner: 'jest', threshold: 50 });
  assert.strictEqual(results[1][1].projects['libs/b'].targets.scoped.options.runner, 'vitest');
});

test('findConfigChange flags untraceable workspace and project config files', () => {
  const { findConfigChange } = executor;
  assert.strictEqual(findConfigChange(['libs/a/src/x.ts', 'README.md'], 'libs/a'), null);
  assert.strictEqual(findConfigChange(['pnpm-lock.yaml'], 'libs/a'), 'pnpm-lock.yaml');
  assert.strictEqual(findConfigChange(['libs/a/jest.config.ts'], 'libs/a'), 'libs/a/jest.config.ts');
  assert.strictEqual(findConfigChange(['libs/a/tsconfig.spec.json'], 'libs/a'), 'libs/a/tsconfig.spec.json');
  // Another project's config, or a nested package.json, does not force a full run.
  assert.strictEqual(findConfigChange(['libs/b/jest.config.ts'], 'libs/a'), null);
  assert.strictEqual(findConfigChange(['libs/a/src/package.json'], 'libs/a'), null);
});

test('filterToProject keeps only the project tests, relative to its root', () => {
  const root = workspace({ 'libs/a/src/a.spec.ts': '', 'libs/ab/src/b.spec.ts': '' });
  const tests = [path.join(root, 'libs/a/src/a.spec.ts'), path.join(root, 'libs/ab/src/b.spec.ts')];
  assert.deepStrictEqual(executor.filterToProject(tests, root, path.join(root, 'libs/a')), ['src/a.spec.ts']);
});

test('resolveBase prefers option, then NX_BASE, then nx.json', () => {
  const { resolveBase } = executor;
  const context = { root: process.cwd(), nxJsonConfiguration: { defaultBase: 'develop' } };
  const saved = process.env.NX_BASE;
  try {
    process.env.NX_BASE = 'abc123';
    assert.strictEqual(resolveBase({ base: 'origin/main' }, context), 'origin/main');
    assert.strictEqual(resolveBase({}, context), 'abc123');
    delete process.env.NX_BASE;
    assert.strictEqual(resolveBase({}, context), 'develop');
  } finally {
    if (saved === undefined) delete process.env.NX_BASE;
    else process.env.NX_BASE = saved;
  }
});

test('shellQuote leaves simple paths alone and quotes the rest', { skip: process.platform === 'win32' }, () => {
  assert.strictEqual(executor.shellQuote('src/a.spec.ts'), 'src/a.spec.ts');
  assert.strictEqual(executor.shellQuote("src/it's here.spec.ts"), `'src/it'\\''s here.spec.ts'`);
});

test('withLock runs callers one at a time and releases the lock', async () => {
  const root = workspace({});
  let active = 0;
  let maxActive = 0;
  const task = () =>
    withLock(root, () => {
      active += 1;
      maxActive = Math.max(maxActive, active);
      active -= 1;
    });
  await Promise.all([task(), task(), task()]);
  assert.strictEqual(maxActive, 1);
  assert.ok(!fs.existsSync(path.join(root, '.scopetest', 'nx.lock')));
});

test('init generator registers the plugin and ignores the cache, idempotently', async () => {
  const files = new Map([
    ['nx.json', '{"plugins":["@nx/jest/plugin"]}'],
    ['.gitignore', 'node_modules'],
  ]);
  const tree = {
    read: (file) => files.get(file),
    write: (file, content) => files.set(file, content),
    exists: (file) => files.has(file),
  };

  await initGenerator(tree, {});
  await initGenerator(tree, {});

  assert.deepStrictEqual(JSON.parse(files.get('nx.json')).plugins, [
    '@nx/jest/plugin',
    { plugin: 'scopetest-nx', options: { targetName: 'test-affected' } },
  ]);
  assert.strictEqual(files.get('.gitignore'), 'node_modules\n.scopetest/\n');
});
