# Quick Start

## Pick a starting path

- Local development: fastest way to verify backend, frontend, and the HTTP facade.
- Single-container POC: fastest way to inspect the published image.
- Docker Compose / ForwardAuth: better fit for long-running or public deployments.

## Prerequisites

- Rust `1.91+`
- Bun `1.3.10` or newer matching the repo pin
- SQLite runtime dependencies for local Rust builds

## Local development

```bash
# Backend; DEV_OPEN_ADMIN is for local debugging only
DEV_OPEN_ADMIN=true cargo run -- --bind 127.0.0.1 --port 58087

# Frontend dev server
cd web
bun install --frozen-lockfile
bun run --bun dev -- --host 127.0.0.1 --port 55173
```

Validation URLs:

- backend health: `http://127.0.0.1:58087/health`
- web console: `http://127.0.0.1:55173`

Notes:

- `DEV_OPEN_ADMIN=true` unlocks admin-only endpoints and lets `/mcp` plus `/api/tavily/*` fall
  back to a local dev token when no token is provided.
- Keep that flag for local validation only. Use ForwardAuth or built-in admin login for any shared
  deployment.

## Seed the first Tavily key

```bash
curl -X POST http://127.0.0.1:58087/api/keys \
  -H "Content-Type: application/json" \
  -d '{"api_key":"tvly-..."}'
```

Confirm it landed in the pool:

```bash
curl http://127.0.0.1:58087/api/keys
```

If you are not using `DEV_OPEN_ADMIN=true`, you must satisfy one of these first:

- the request already comes through a ForwardAuth gateway with matching
  `FORWARD_AUTH_HEADER` / `FORWARD_AUTH_ADMIN_VALUE`
- you enabled built-in admin login and completed that login in the browser
- you are doing local header-based testing and explicitly configured
  `FORWARD_AUTH_HEADER=X-Forwarded-User` plus
  `FORWARD_AUTH_ADMIN_VALUE=admin@example.com`

## Verify the HTTP facade immediately

With `DEV_OPEN_ADMIN=true`, you can send a first search request without a token:

```bash
curl -X POST http://127.0.0.1:58087/api/tavily/search \
  -H "Content-Type: application/json" \
  -d '{
    "query": "rust async runtime",
    "topic": "general",
    "search_depth": "basic",
    "max_results": 3
  }'
```

If you are already in normal auth mode with a real user token, prefer:

```bash
curl -X POST http://127.0.0.1:58087/api/tavily/search \
  -H "Authorization: Bearer th-<id>-<secret>" \
  -H "Content-Type: application/json" \
  -d '{"query":"rust async runtime","topic":"general"}'
```

Once that works, you have verified:

- the service is up
- an upstream Tavily key is registered
- Hikari can proxy a real Tavily HTTP request end to end

## Install the CLI wrapper

After you have a Hikari user token, install the GitHub Release wrapper:

```bash
curl -fsSL "https://github.com/IvanLi-CN/tavily-hikari/releases/latest/download/install-tvly-hikari.sh" | bash -s -- \
  --base-url "http://127.0.0.1:58087" \
  --token "th-<id>-<secret>"
```

Then use official Tavily CLI commands through Hikari:

```bash
tvly-hikari search "rust async runtime" --json
```

Optional Agent Skills install:

```bash
npx skills add https://github.com/IvanLi-CN/tavily-hikari
```

The CLI stores the Hikari token with `0600` permissions and sends traffic to
`<base-url>/api/tavily`. Do not use an official Tavily API key as the downstream token.

## Single-container POC

```bash
docker run --rm \
  -p 8787:8787 \
  -e PROXY_BIND=0.0.0.0 \
  -e DEV_OPEN_ADMIN=true \
  -v "$(pwd)/data:/srv/app/data" \
  ghcr.io/ivanli-cn/tavily-hikari:latest
```

The image serves the bundled `web/dist` assets and writes SQLite data to
`/srv/app/data/tavily_proxy.db`.

Validate it the same way as local dev, just against `http://127.0.0.1:8787`:

```bash
curl http://127.0.0.1:8787/health
curl -X POST http://127.0.0.1:8787/api/keys \
  -H "Content-Type: application/json" \
  -d '{"api_key":"tvly-..."}'
```

This remains a POC path because it relies on `DEV_OPEN_ADMIN=true`.

## Docker Compose

```bash
docker compose up -d
```

The stock `docker-compose.yml`:

- exposes `8787`
- uses the `tavily-hikari-data` volume
- runs `ghcr.io/ivanli-cn/tavily-hikari:latest`
- persists SQLite at `/srv/app/data/tavily_proxy.db`

It only starts Hikari itself, so you still need one admin strategy before real admin work:

- temporary local validation: add `DEV_OPEN_ADMIN=true`
- self-hosted mode: enable built-in admin login
- formal gateway mode: switch to the
  [ForwardAuth + Caddy example](https://github.com/IvanLi-CN/tavily-hikari/tree/main/examples/forwardauth-caddy)

## Need a public admin entrypoint? Use the ForwardAuth example

If you are ready to test a gateway flow, start from the repository example instead of building one
from scratch:

```bash
cd examples/forwardauth-caddy
docker compose up -d
```

That stack launches:

- Caddy as the reverse proxy
- `auth-mock` as a ForwardAuth simulator
- `upstream-mock` as a Tavily upstream simulator
- Tavily Hikari itself

Its goal is to prove the gateway, identity-header, and Hikari chain before you swap in your real
auth provider and real upstream.

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

## Next

- Need the full auth, built-in admin, or Linux DO OAuth setup:
  [Configuration & Access](/configuration-access)
- Need full request examples for `/api/tavily/*`: [HTTP API Guide](/http-api-guide)
- Need production and high-anonymity guidance: [Deployment & Anonymity](/deployment-anonymity)
- Already running, but stuck on 401, 502, admin access, or persistence:
  [FAQ & Troubleshooting](/faq)
