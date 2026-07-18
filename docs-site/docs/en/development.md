# Development

## Repository layout

- `src/`: Rust backend, router, services, CLI
- `web/`: React + Vite app and Storybook
- `docs-site/`: public Rspress docs site
- `docs/`: internal design docs, specs, and historical planning material

## Core commands

### Backend

```bash
cargo fmt
cargo clippy -- -D warnings
cargo test
```

### Frontend

```bash
cd web
bun install --frozen-lockfile
bun test
bun run build
bun run build-storybook
```

### Docs-site

```bash
cd docs-site
bun install --frozen-lockfile
bun run build
```

## Review surfaces

- Runtime app: backend + Vite dev server
- Storybook: component, fragment, and page-level browseable UI review
- Docs-site: operator-facing product and deployment reference

The GitHub Pages workflow builds docs-site and Storybook separately, then assembles them into a
single static artifact with Storybook mounted under `/storybook/`.

## CI and release

- `main` now follows a repo-local `quality-gates` contract: PR-only for everyone, strict required checks, signed commits, no force-push, no branch deletion, and admin enforcement enabled.
- The merge contract keeps the following required checks fixed:
  - `Quality Gates Contract`
  - `Release intent label gate`
  - `Worktree Bootstrap Smoke`
  - `Web Assets`
  - `Lint & Checks`
  - `Backend Shard Plan`
  - `Backend Tests`
  - `Frontend Checks`
  - `Compose Smoke (ForwardAuth + Caddy)`
  - `Build (Release)`
  - `Docs Pages Gate`
- `Docs Pages Gate` is the only required docs-surface check. `build-docs`, `build-storybook`, and `assemble-pages` stay visible as leaf checks for failure triage; when a PR is unrelated to docs/site/Storybook, the gate returns success/no-op instead of disappearing.
- `CI Pipeline` handles Rust checks, backend tests, and compose smoke coverage.
- `Docs Pages` now evaluates scope on every PR and `main` push; when docs/web/README/root `.bun-version`/assemble inputs are relevant it builds docs-site + Storybook and deploys Pages, otherwise it only emits the no-op `Docs Pages Gate` success.
- `Release` publishes container releases based on PR intent labels.

## Owner-side drift audit

After authenticating `gh` on a maintainer machine, run:

```bash
python3 .github/scripts/check_quality_gates.py --github-live IvanLi-CN/tavily-hikari --github-branch main
```

This validates:

- the repo-local `.github/quality-gates.json` schema and workflow inventory
- GitHub `main` branch protection required checks / strict mode / admin enforcement / force-push / deletion state
- GitHub `main` required signatures (signed commits)

To regenerate the branch-protection payload before syncing with `gh api`, run:

```bash
python3 .github/scripts/check_quality_gates.py --emit-branch-protection-payload
```
