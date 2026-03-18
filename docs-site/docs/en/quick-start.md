# Quick Start

## Prerequisites

- Rust `1.91+`
- Bun `1.3.10` or newer matching the repo pin
- SQLite runtime dependencies for local Rust builds

## Local backend + frontend

```bash
# Backend
cargo run -- --bind 127.0.0.1 --port 58087

# Frontend dev server
cd web
bun install --frozen-lockfile
bun run --bun dev -- --host 127.0.0.1 --port 55173
```

Open `http://127.0.0.1:58087/health` for the backend probe and `http://127.0.0.1:55173` for the
web console.

## Seed a Tavily key

```bash
curl -X POST http://127.0.0.1:58087/api/keys \
  -H "X-Forwarded-User: admin@example.com" \
  -H "X-Forwarded-Admin: true" \
  -H "Content-Type: application/json" \
  -d '{"api_key":"key_a"}'
```

For local development, this request usually works behind ForwardAuth headers or with
`DEV_OPEN_ADMIN=true`.

## Docker

```bash
docker run --rm \
  -p 8787:8787 \
  -v "$(pwd)/data:/srv/app/data" \
  ghcr.io/ivanli-cn/tavily-hikari:latest
```

The image serves the bundled `web/dist` assets and writes SQLite data to
`/srv/app/data/tavily_proxy.db`.

## Docker Compose

```bash
docker compose up -d
```

The repository `docker-compose.yml` exposes port `8787` and uses a named volume for persistent
data. See the repository root for the compose file and the
[ForwardAuth + Caddy example](https://github.com/IvanLi-CN/tavily-hikari/tree/main/examples/forwardauth-caddy)
for a gateway-oriented deployment sample.

## Optional local review surfaces

```bash
# Storybook
cd web
bun install --frozen-lockfile
bun run storybook

# Public docs site
cd docs-site
bun install --frozen-lockfile
bun run dev
```

- Storybook default local URL: `http://127.0.0.1:56006`
- Docs-site default local URL: `http://127.0.0.1:56007`

The docs-site and Storybook are designed to cross-link in local preview and in the final GitHub
Pages deployment.
