# Web (Vite + React)

## Quick Start

```bash
bun install --frozen-lockfile
bun run --bun dev -- --host 127.0.0.1 --port 55173
```

Open `http://127.0.0.1:55173`.

## Storybook

```bash
bun install --frozen-lockfile
bun run storybook
# Optional: prove frontend scripts do not require the system `node` binary
cd .. && bun run validate:no-node-runtime
```

Open `http://127.0.0.1:56006`.

When Storybook is opened from the public docs-site, it remembers that docs origin and rewrites
docs links back to that site. You can override the target manually with `VITE_DOCS_SITE_ORIGIN`.

## Docs Site

```bash
cd ../docs-site
bun install --frozen-lockfile
bun run dev
```

Open `http://127.0.0.1:56007`.

- `DOCS_PORT` overrides the local docs-site port.
- `VITE_STORYBOOK_DEV_ORIGIN` lets the docs-site redirect page target a non-default Storybook dev
  origin.
- Static Storybook output is assembled into the public docs-site under `/storybook/`, with entry
  pages exposed at `/storybook.html` and `/storybook-guide.html`.
