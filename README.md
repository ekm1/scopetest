# scopetest-cli

A blazing-fast smart test selector for JavaScript/TypeScript monorepos. Only run tests affected by your changes.

## Features

- 🚀 **Fast** - Built in Rust, parses 12,000+ files in seconds
- 🎯 **Accurate** - Tracks transitive dependencies through barrel files
- 📦 **Monorepo-aware** - Follows workspace package symlinks
- 🔧 **Jest-compatible** - Outputs `--testPathPattern` for Jest
- 💾 **Cached** - Incremental updates for even faster subsequent runs

## Installation

```bash
npm install -g scopetest-cli
# or
npx scopetest-cli affected --base main
```

## Quick Start

```bash
# Find tests affected by changes on current branch vs main
scopetest affected --base main

# Output as Jest pattern (default)
scopetest affected --base main --format jest
# Output: Button\.spec\.ts|Header\.spec\.ts

# Run only affected tests
jest --testPathPattern="$(scopetest affected --base main)"

# JSON output with stats
scopetest affected --base main --format json
```

## Commands

### `affected`

Find tests affected by changes between current branch and base.

```bash
scopetest affected [OPTIONS]

Options:
  -b, --base <REF>      Base branch/commit to compare against
  -f, --format <FMT>    Output format: jest, json, list [default: jest]
      --no-cache        Skip cache, force full rebuild
  -r, --root <PATH>     Project root directory
```

### `build`

Build/rebuild the dependency graph cache.

```bash
scopetest build [OPTIONS]

Options:
  -r, --root <PATH>     Project root directory
```

### `coverage`

Output coverage scope for affected files.

```bash
scopetest coverage [OPTIONS]

Options:
  -b, --base <REF>      Base branch/commit
  -f, --format <FMT>    Output format: list, json [default: list]
```

## Configuration

Create `.scopetestrc.json` in your project root:

```json
{
  "testPatterns": [
    "**/*.spec.ts",
    "**/*.spec.tsx",
    "**/*.test.ts",
    "**/*.test.tsx"
  ],
  "ignorePatterns": [
    "**/node_modules/**",
    "**/dist/**",
    "**/build/**",
    "**/.git/**"
  ],
  "extensions": [".ts", ".tsx", ".js", ".jsx"]
}
```

## CI Integration

### GitHub Actions

```yaml
- name: Run affected tests
  run: |
    AFFECTED=$(npx scopetest-cli affected --base origin/main)
    if [ -n "$AFFECTED" ] && [ "$AFFECTED" != "^$" ]; then
      jest --testPathPattern="$AFFECTED"
    else
      echo "No affected tests"
    fi
```

### Jenkins / Other CI

```bash
AFFECTED=$(npx scopetest-cli affected --base origin/master)
if [ -n "$AFFECTED" ] && [ "$AFFECTED" != "^$" ]; then
  npm test -- --testPathPattern="$AFFECTED"
fi
```

## How It Works

1. **Parse** - Uses [oxc](https://oxc.rs) to parse all JS/TS files and extract imports
2. **Graph** - Builds a dependency graph using [petgraph](https://docs.rs/petgraph)
3. **Diff** - Gets changed files from `git diff`
4. **Traverse** - Finds all files that transitively depend on changed files
5. **Filter** - Returns only test files from the affected set

## Performance

Tested on a real monorepo with 12,500+ files:
- Initial build: ~3 seconds
- Cached run: ~200ms

## Supported Import Syntax

- ES6 imports: `import x from 'y'`, `import { x } from 'y'`
- Dynamic imports: `import('path')`
- CommonJS: `require('path')`
- Re-exports: `export * from 'y'`, `export { x } from 'y'`
- Parent directory imports: `import { x } from '..'`
- TypeScript path aliases via `tsconfig.json`
- Workspace package resolution via symlinks

## License

MIT
