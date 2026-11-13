'use strict';

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const LOCK_STALE_MS = 2 * 60 * 1000;
const LOCK_POLL_MS = 50;

/** Locate the scopetest launcher: $SCOPETEST_BIN, the installed scopetest-cli, then PATH. */
function resolveScopetest(workspaceRoot) {
  if (process.env.SCOPETEST_BIN) {
    return { command: process.env.SCOPETEST_BIN, prefixArgs: [] };
  }
  try {
    const launcher = require.resolve('scopetest-cli/bin/scopetest', {
      paths: [workspaceRoot, __dirname],
    });
    return { command: process.execPath, prefixArgs: [launcher] };
  } catch {
    return { command: 'scopetest', prefixArgs: [] };
  }
}

/** Resolve the JS entry of a package binary (e.g. jest) as seen from the given directories. */
function resolveBin(pkg, binName, fromDirs) {
  let pkgJsonPath;
  try {
    pkgJsonPath = require.resolve(`${pkg}/package.json`, { paths: fromDirs });
  } catch {
    return null;
  }
  const bin = JSON.parse(fs.readFileSync(pkgJsonPath, 'utf8')).bin;
  const rel = typeof bin === 'string' ? bin : bin && bin[binName];
  return rel ? path.join(path.dirname(pkgJsonPath), rel) : null;
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Serialize scopetest runs across the parallel tasks of one `nx run-many`, so a
 * cold dependency-graph cache is built once instead of once per project.
 */
async function withLock(workspaceRoot, fn) {
  const cacheDir = path.join(workspaceRoot, '.scopetest');
  const lockDir = path.join(cacheDir, 'nx.lock');
  fs.mkdirSync(cacheDir, { recursive: true });

  for (;;) {
    try {
      fs.mkdirSync(lockDir);
      break;
    } catch (err) {
      if (err.code !== 'EEXIST') throw err;
      try {
        if (Date.now() - fs.statSync(lockDir).mtimeMs > LOCK_STALE_MS) {
          fs.rmSync(lockDir, { recursive: true, force: true });
          continue;
        }
      } catch {
        continue; // released between mkdir and stat
      }
      await sleep(LOCK_POLL_MS);
    }
  }

  try {
    return fn();
  } finally {
    fs.rmSync(lockDir, { recursive: true, force: true });
  }
}

/** Ask scopetest for every affected test file in the workspace (absolute paths). */
async function findAffectedTests(workspaceRoot, { base, noCache }) {
  const { command, prefixArgs } = resolveScopetest(workspaceRoot);
  const args = [...prefixArgs, 'affected', '--format', 'json', '--root', workspaceRoot, '--base', base];
  if (noCache) args.push('--no-cache');

  const result = await withLock(workspaceRoot, () =>
    spawnSync(command, args, {
      cwd: workspaceRoot,
      encoding: 'utf8',
      maxBuffer: 256 * 1024 * 1024,
      stdio: ['ignore', 'pipe', 'pipe'],
    })
  );

  if (result.error) {
    throw new Error(
      `Could not run scopetest (${result.error.message}). Install scopetest-cli or set SCOPETEST_BIN.`
    );
  }
  if (result.status !== 0) {
    throw new Error(`scopetest failed: ${(result.stderr || '').trim() || `exit code ${result.status}`}`);
  }
  return JSON.parse(result.stdout).tests;
}

module.exports = { findAffectedTests, resolveBin, resolveScopetest, withLock };
