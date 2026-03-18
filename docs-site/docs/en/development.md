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

- `CI Pipeline` handles Rust checks, backend tests, and compose smoke coverage.
- `Docs Pages` builds docs-site + Storybook and deploys them to GitHub Pages on `main`.
- `Release` publishes container releases based on PR intent labels.
