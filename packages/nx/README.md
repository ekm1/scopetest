# nx-scopetest

Nx plugin for [scopetest](https://github.com/ekm1/scopetest). Nx decides *which projects* are affected; scopetest decides *which test files* inside them are — so a one-line change in a shared lib runs a handful of spec files instead of every suite downstream.

## Setup

```bash
npx nx add nx-scopetest
```

That installs the package (the scopetest binary comes with it as a dependency) and runs its `init` generator. The manual equivalent:

```bash
npm install -D nx-scopetest
npx nx g nx-scopetest:init
```

`init` registers the plugin in `nx.json` and git-ignores the `.scopetest/` cache. Every project with a `jest.config.*`, `vitest.config.*` (or a `vite.config.*` containing a `test` block) now has a `test-affected` target — no `project.json` edits needed.

## Usage

```bash
# One project
nx test-affected my-app

# Everything, in parallel - projects with no affected tests finish instantly
nx run-many -t test-affected

# Let Nx narrow the projects first
nx affected -t test-affected

# Against a specific ref, with extra runner flags
nx test-affected my-app --base=origin/main --args=--coverage
```

The base ref is resolved in this order: `--base`, `NX_BASE` (set by `nrwl/nx-set-shas`), `defaultBase` in `nx.json`, then `main`/`master`.

### GitHub Actions

```yaml
- uses: actions/checkout@v4
  with:
    fetch-depth: 0
- uses: nrwl/nx-set-shas@v4
- run: npx nx affected -t test-affected
```

## Options

Set workspace-wide defaults in the plugin entry of `nx.json`, or per project/run as executor options.

```json
{
  "plugins": [
    {
      "plugin": "nx-scopetest",
      "options": { "targetName": "test-affected", "threshold": 200 }
    }
  ]
}
```

| Option | Default | Description |
|--------|---------|-------------|
| `targetName` *(plugin only)* | `test-affected` | Name of the inferred target |
| `runner` | detected | `jest`, `vitest` or `auto` |
| `base` | see above | Git ref to compare against |
| `args` | `[]` | Extra arguments for the test runner |
| `command` | – | Custom command run from the project root; `{}` is replaced by the affected files. Replaces the built-in runner |
| `threshold` | – | If more test files than this are affected in a project, run its whole suite |
| `runAllOnConfigChange` | `true` | Run the project's whole suite when a file the import graph cannot trace changed: lockfiles, root `package.json`, `nx.json`, `tsconfig.base.json`, `jest.preset.*`, or the project's own `project.json` / `package.json` / `tsconfig*.json` / runner config |
| `noCache` | `false` | Rebuild the dependency graph instead of using the cache |

To wire a target by hand instead of using the inferred one:

```json
{
  "targets": {
    "test-affected": {
      "executor": "nx-scopetest:affected",
      "options": { "command": "npx playwright test {}" }
    }
  }
}
```

## Compatibility

| | Supported | Tested with |
|---|---|---|
| Nx | 19.2 and later | 19.2.0, 20.8, 21.6, 22.7, 23.3, 24.0 |
| Jest | 29, 30 | 29.7, 30.5 |
| Vitest | 1 and later | 1.6, 2.1, 3.2, 5.1 |
| Node.js | 18 and later | 22, 24 |
| scopetest-cli | 0.2.2 and later | 0.2.2 |
| OS / arch | macOS (arm64, x64), Linux (x64, arm64), Windows (x64) | Linux x64 |

Nx 19.2 is the minimum because the plugin uses the `createNodesV2` API introduced there. Platforms are those `scopetest-cli` ships a binary for.

Example setups known to work together:

```jsonc
// Older workspace
{ "nx": "19.2.0", "jest": "^29.7.0", "vitest": "^1.6.0" }

// Mid-range
{ "nx": "^21.6.0", "jest": "^29.7.0", "vitest": "^3.2.0" }

// Current
{ "nx": "^24.0.0", "jest": "^30.5.0", "vitest": "^5.1.0", "vite": "^8.0.0" }
```

Other runners (Playwright, Mocha, …) work through the `command` option.

## Notes

- The runner is started in the project root with the affected files (`jest --runTestsByPath …` / `vitest run …`), so it picks up the project's own config.
- Path aliases are read from `tsconfig.base.json` and `tsconfig.json` at the workspace root; workspace packages linked into `node_modules` are followed too.
- Test file patterns and ignores come from `.scopetestrc.json` at the workspace root.
- The target is never cached by Nx — its result depends on git state. Keep your regular `test` target for cached full runs.
- Set `SCOPETEST_BIN` to use a specific scopetest binary instead of the installed `scopetest-cli`.
