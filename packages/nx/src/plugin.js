'use strict';

const fs = require('fs');
const path = require('path');

const CONFIG_GLOB = '**/{jest,vitest,vite}.config.{js,ts,mjs,cjs,mts,cts}';
const DEFAULT_TARGET_NAME = 'test-affected';

/**
 * Infers a `test-affected` target for every project that has a Jest or Vitest
 * config, so `nx test-affected <project>` and `nx affected -t test-affected`
 * work without touching any project.json.
 */
const createNodesV2 = [
  CONFIG_GLOB,
  (configFiles, options, context) => {
    const byRoot = new Map();
    for (const configFile of configFiles) {
      const runner = detectRunner(configFile, context.workspaceRoot);
      if (!runner) continue;
      const root = path.posix.dirname(configFile.split(path.sep).join('/'));
      if (!isProject(root, context.workspaceRoot)) continue;
      // A project with both configs keeps whichever sorts first (jest).
      const existing = byRoot.get(root);
      if (!existing || runner < existing.runner) {
        byRoot.set(root, { configFile, runner });
      }
    }

    const targetName = (options && options.targetName) || DEFAULT_TARGET_NAME;
    const results = [];
    for (const [root, { configFile, runner }] of byRoot) {
      results.push([
        configFile,
        { projects: { [root]: { targets: { [targetName]: buildTarget(runner, options) } } } },
      ]);
    }
    return results;
  },
];

function buildTarget(runner, options) {
  const targetOptions = { runner };
  for (const key of ['base', 'threshold', 'runAllOnConfigChange']) {
    if (options && options[key] !== undefined) targetOptions[key] = options[key];
  }
  return {
    executor: 'scopetest-nx:affected',
    // The result depends on git state, which Nx inputs cannot describe.
    cache: false,
    options: targetOptions,
    metadata: {
      technologies: ['scopetest', runner],
      description: `Run the ${runner} tests of this project affected by changes since the base ref`,
    },
  };
}

function detectRunner(configFile, workspaceRoot) {
  const name = path.basename(configFile);
  if (name.startsWith('jest.config.')) return 'jest';
  if (name.startsWith('vitest.config.')) return 'vitest';
  // vite.config.* only counts when it carries a Vitest `test` block.
  try {
    const content = fs.readFileSync(path.join(workspaceRoot, configFile), 'utf8');
    return /\btest\s*:/.test(content) ? 'vitest' : null;
  } catch {
    return null;
  }
}

function isProject(root, workspaceRoot) {
  const dir = path.join(workspaceRoot, root);
  return (
    fs.existsSync(path.join(dir, 'project.json')) || fs.existsSync(path.join(dir, 'package.json'))
  );
}

module.exports = { createNodesV2, detectRunner, DEFAULT_TARGET_NAME };
