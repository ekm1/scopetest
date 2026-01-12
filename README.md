# scopetest

[![npm version](https://img.shields.io/npm/v/scopetest-cli.svg)](https://www.npmjs.com/package/scopetest-cli)
[![npm downloads](https://img.shields.io/npm/dm/scopetest-cli.svg)](https://www.npmjs.com/package/scopetest-cli)
[![license](https://img.shields.io/npm/l/scopetest-cli.svg)](https://github.com/ekm1/scopetest/blob/main/LICENSE)

Run only the tests that matter. A fast, dependency-aware test selector for JS/TS monorepos.

```bash
# Run affected tests with one command
scopetest affected -x "jest --runTestsByPath {} --colors"
```

## Why?

In large monorepos, running all tests is slow. `scopetest` analyzes your dependency graph and runs only tests affected by your changes—cutting CI time from minutes to seconds.

## Install

```bash
npm install -g scopetest-cli
```

## Usage

```bash
# Find affected tests
scopetest affected --base main

# Execute tests directly
scopetest affected -x "jest --runTestsByPath {}"
scopetest affected -x "vitest run {}"

# Get affected source files instead
scopetest affected --sources

# Different output formats
scopetest affected -f list    # newline-separated
scopetest affected -f json    # full stats
```

### Commands

**`affected`** - Find tests affected by changes

```
Options:
  -b, --base <REF>     Git ref to compare against (branch, commit, tag)
  -f, --format <FMT>   Output: paths, list, json [default: paths]
  -x, --exec <CMD>     Execute command with {} replaced by affected files
      --sources        Output affected source files instead of tests
      --no-cache       Skip cache, force rebuild
  -r, --root <PATH>    Project root directory
```

**`build`** - Rebuild dependency graph cache

```
Options:
  -r, --root <PATH>    Project root directory
```

## Output Formats

| Format | Description | Example |
|--------|-------------|---------|
| `paths` | Space-separated (default) | `src/a.spec.ts src/b.spec.ts` |
| `list` | Newline-separated | `src/a.spec.ts`<br>`src/b.spec.ts` |
| `json` | Full stats | `{"tests": [...], "stats": {...}}` |

Aliases: `jest` and `vitest` both map to `paths`.

## Configuration

Create `.scopetestrc.json`:

```json
{
  "testPatterns": ["**/*.spec.ts", "**/*.test.ts"],
  "ignorePatterns": ["**/node_modules/**", "**/dist/**"],
  "extensions": [".ts", ".tsx", ".js", ".jsx"]
}
```

## CI Examples

### GitHub Actions

```yaml
- name: Run affected tests
  run: npx scopetest-cli affected -b origin/main -x "jest --runTestsByPath {} --colors"
```

### Jenkins

```bash
npx scopetest-cli affected -b origin/master -x "npx jest --runTestsByPath {} --colors"
```

### Manual (pipe style)

```bash
jest --runTestsByPath $(scopetest affected -b main)
```

## How It Works

1. Parses all JS/TS files using [oxc](https://oxc.rs)
2. Builds a dependency graph with [petgraph](https://docs.rs/petgraph)
3. Gets changed files from `git diff`
4. Traverses graph to find all affected files
5. Filters to test files only

## Performance

On a 12,500+ file monorepo:
- Initial build: ~3s
- Cached: ~200ms

## Supported Imports

- ES6: `import x from 'y'`
- Dynamic: `import('path')`
- CommonJS: `require('path')`
- Re-exports: `export * from 'y'`
- TypeScript path aliases
- Workspace packages

## License

MIT
