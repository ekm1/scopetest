'use strict';

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');
const { findAffectedTests, resolveBin } = require('../../lib/scopetest');

// Workspace-root files whose changes scopetest's import graph cannot see.
const WORKSPACE_CONFIG_FILES = new Set([
  'package.json',
  'package-lock.json',
  'yarn.lock',
  'pnpm-lock.yaml',
  'bun.lock',
  'bun.lockb',
  'nx.json',
  'tsconfig.base.json',
  'jest.preset.js',
  'jest.preset.cjs',
  'jest.preset.mjs',
  'jest.preset.ts',
  '.scopetestrc.json',
]);
const PROJECT_CONFIG_FILE =
  /^(project\.json|package\.json|tsconfig(\..+)?\.json|(jest|vitest|vite)\.config\.[cm]?[jt]s)$/;

module.exports = async function affectedExecutor(options, context) {
  const workspaceRoot = context.root;
  const projectRoot = context.projectsConfigurations.projects[context.projectName].root;
  const projectDir = path.join(workspaceRoot, projectRoot);
  const label = `[scopetest] ${context.projectName}:`;

  let base;
  let runAll = null;
  let tests = [];
  try {
    base = resolveBase(options, context);
    if (options.runAllOnConfigChange !== false) {
      const trigger = findConfigChange(changedFiles(workspaceRoot, base), projectRoot);
      if (trigger) runAll = `${trigger} changed`;
    }
    if (!runAll) {
      const all = await findAffectedTests(workspaceRoot, { base, noCache: options.noCache });
      tests = filterToProject(all, workspaceRoot, projectDir);
      if (options.threshold !== undefined && tests.length > options.threshold) {
        runAll = `${tests.length} affected test files exceed the threshold of ${options.threshold}`;
      }
    }
  } catch (err) {
    console.error(`${label} ${err.message}`);
    return { success: false };
  }

  if (!runAll && tests.length === 0) {
    console.log(`${label} no tests affected by changes since ${base}.`);
    return { success: true };
  }
  console.log(
    runAll
      ? `${label} ${runAll} - running all tests.`
      : `${label} running ${tests.length} affected test file(s) (base: ${base}).`
  );

  const files = runAll ? [] : tests;
  const status = options.command
    ? runCustomCommand(options.command, files, projectDir)
    : runRunner(options, files, projectDir, workspaceRoot, label);
  return { success: status === 0 };
};

function resolveBase(options, context) {
  const nxJson = context.nxJsonConfiguration || {};
  const configured =
    options.base ||
    process.env.NX_BASE ||
    nxJson.defaultBase ||
    (nxJson.affected && nxJson.affected.defaultBase);
  if (configured) return configured;
  return refExists(context.root, 'main') ? 'main' : 'master';
}

function refExists(cwd, ref) {
  return spawnSync('git', ['rev-parse', '--verify', '--quiet', ref], { cwd }).status === 0;
}

function changedFiles(workspaceRoot, base) {
  const result = spawnSync('git', ['diff', '--name-only', base], {
    cwd: workspaceRoot,
    encoding: 'utf8',
    maxBuffer: 256 * 1024 * 1024,
  });
  if (result.error || result.status !== 0) {
    const detail = result.error ? result.error.message : (result.stderr || '').trim();
    throw new Error(
      `git diff against "${base}" failed: ${detail}\n` +
        'Make sure the base ref is fetched (e.g. `fetch-depth: 0` in CI) or pass --base.'
    );
  }
  return result.stdout.split('\n').filter(Boolean);
}

/** Returns the first changed file that should force a full run of this project, if any. */
function findConfigChange(files, projectRoot) {
  const prefix = projectRoot === '.' ? '' : `${projectRoot.replace(/\/+$/, '')}/`;
  for (const file of files) {
    if (!file.includes('/') && WORKSPACE_CONFIG_FILES.has(file)) return file;
    if (prefix && file.startsWith(prefix)) {
      const rest = file.slice(prefix.length);
      if (!rest.includes('/') && PROJECT_CONFIG_FILE.test(rest)) return file;
    }
  }
  return null;
}

/** Keep the tests inside the project, as POSIX paths relative to the project root. */
function filterToProject(tests, workspaceRoot, projectDir) {
  const realProject = realpath(projectDir);
  const result = [];
  for (const test of tests) {
    const abs = realpath(path.resolve(workspaceRoot, test));
    const rel = path.relative(realProject, abs);
    if (rel && !rel.startsWith('..') && !path.isAbsolute(rel)) {
      result.push(rel.split(path.sep).join('/'));
    }
  }
  return result;
}

function realpath(p) {
  try {
    return fs.realpathSync(p);
  } catch {
    return p;
  }
}

function detectRunner(projectDir, workspaceRoot) {
  const entries = fs.readdirSync(projectDir);
  if (entries.some((e) => e.startsWith('jest.config.'))) return 'jest';
  if (entries.some((e) => e.startsWith('vitest.config.') || e.startsWith('vite.config.'))) {
    return 'vitest';
  }
  const dirs = [projectDir, workspaceRoot];
  if (resolveBin('jest', 'jest', dirs)) return 'jest';
  if (resolveBin('vitest', 'vitest', dirs)) return 'vitest';
  return null;
}

function runRunner(options, files, projectDir, workspaceRoot, label) {
  const runner =
    !options.runner || options.runner === 'auto'
      ? detectRunner(projectDir, workspaceRoot)
      : options.runner;
  if (!runner) {
    console.error(`${label} could not detect a test runner. Set the "runner" or "command" option.`);
    return 1;
  }
  const bin = resolveBin(runner, runner, [projectDir, workspaceRoot]);
  if (!bin) {
    console.error(`${label} ${runner} is not installed in this workspace.`);
    return 1;
  }

  const extra = options.args || [];
  let args;
  if (runner === 'jest') {
    args = files.length ? ['--runTestsByPath', ...files, ...extra] : extra;
  } else {
    args = ['run', ...files, ...extra];
  }
  return exitCode(spawnSync(process.execPath, [bin, ...args], { cwd: projectDir, stdio: 'inherit' }));
}

function runCustomCommand(template, files, projectDir) {
  const command = template.replace(/\{\}/g, files.map(shellQuote).join(' '));
  return exitCode(spawnSync(command, { cwd: projectDir, stdio: 'inherit', shell: true }));
}

function shellQuote(value) {
  if (/^[\w@%+=:,./-]+$/.test(value)) return value;
  return process.platform === 'win32'
    ? `"${value.replace(/"/g, '\\"')}"`
    : `'${value.replace(/'/g, `'\\''`)}'`;
}

function exitCode(result) {
  if (result.error) {
    console.error(result.error.message);
    return 1;
  }
  return result.status === null ? 1 : result.status;
}

module.exports.findConfigChange = findConfigChange;
module.exports.filterToProject = filterToProject;
module.exports.resolveBase = resolveBase;
module.exports.shellQuote = shellQuote;
