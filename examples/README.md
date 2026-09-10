# Examples

Two small monorepos used by the integration tests in
[`tests/examples.rs`](../tests/examples.rs). Each one is a real workspace, so
you can also run `scopetest` against them by hand.

| Example | Layout | Package discovery | Aliases |
|---------|--------|-------------------|---------|
| [`turborepo/`](turborepo) | `packages/*`, `apps/*` | `workspaces` in package.json | `tsconfig.json` |
| [`nx/`](nx) | `libs/*`, `apps/*` | `project.json` files | `tsconfig.base.json` |

## Try it

```bash
cd examples/turborepo
git init . && git add -A && git commit -m init

scopetest init
echo "export const VARIANT = 'primary';" >> packages/ui/src/button.ts

scopetest affected --base HEAD --packages -f list
# @acme/ui
# @acme/web
```

`@acme/admin` and `@acme/utils` are correctly left out — nothing in them
reaches the changed file.

## Dependency shape

**turborepo** — `web` depends on `@acme/ui`; `admin` depends on `@acme/utils`.
The two halves are independent, so a change to one never selects the other.

**nx** — `shell` depends on `ui`; `checkout` depends on `ui` and `data`. A
change to `data` selects `checkout` but not `shell`.

Both examples import through a barrel (`src/index.ts`). Barrel expansion means
a change to an export that nobody imports — `libs/ui/src/spinner.ts` in the Nx
example — stays inside its own package instead of fanning out to every
consumer of the barrel.
