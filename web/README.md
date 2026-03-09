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
