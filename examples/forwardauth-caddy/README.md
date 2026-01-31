# forwardauth-caddy

This example shows a simple Caddy gateway protecting Tavily Hikari with `forward_auth`.

## What you get

- Caddy listens on host port `8787`.
- `GET /health` is public.
- Everything else requires Basic Auth checked by `auth-mock`.
- On auth success, `auth-mock` returns:
  - `Remote-Email: admin@example.com`
  - `Remote-Name: admin`
    and Caddy forwards these to Hikari (after clearing any inbound values).
- Hikari is configured to treat `Remote-Email=admin@example.com` as admin and display `Remote-Name` as nickname.
- Upstream calls are pointed to `upstream-mock` (no production Tavily traffic).

## Run

```bash
cd examples/forwardauth-caddy
docker compose up -d

# If your Docker doesn't ship the `docker compose` plugin, use:
docker-compose up -d
```

## CI / local image override

For CI (or local testing), you can override the Hikari image without editing the compose file:

```bash
export TAVILY_HIKARI_IMAGE=tavily-hikari:ci
docker compose up -d
```

## Quick checks

Public:

```bash
curl -i http://127.0.0.1:8787/health
```

Protected (expects `401` + `WWW-Authenticate`):

```bash
curl -i http://127.0.0.1:8787/
```

Protected (success; Basic Auth is `admin:password`):

```bash
curl -i -u admin:password http://127.0.0.1:8787/
```
