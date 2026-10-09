# FAQ & Troubleshooting

## FAQ

### How is Tavily Hikari related to Tavily itself

Tavily Hikari is not a replacement for Tavily. It is the proxy and control layer you run in front
of your own Tavily keys.

It is responsible for:

- serving both `/mcp` and `/api/tavily/*`
- issuing Hikari-owned `th-<id>-<secret>` tokens to downstream users
- routing, auditing, and quota control across multiple upstream Tavily keys

The real Tavily API keys still belong to you and remain under admin control.

### What is the difference between a Hikari token and a Tavily API key

- Hikari token: handed to end users, scripts, HTTP clients, or MCP clients
- Tavily API key: stays in admin-only storage and is only used on upstream requests

If you want key-pool routing, request auditing, and quota enforcement, point clients at Tavily
Hikari instead of exposing raw Tavily keys directly.

### When should I use `/mcp` versus `/api/tavily/*`

- `/mcp`: for Codex CLI, Cursor, Claude Desktop, VS Code, and other MCP clients
- `/api/tavily/*`: for Cherry Studio, scripts, and plain HTTP integrations

If the client setup only asks for a `Base URL + API key`, it usually belongs on `/api/tavily/*`.

### How do I use Tavily CLI or Agent Skills through Hikari

Install the Release wrapper:

```bash
curl -fsSL "https://github.com/IvanLi-CN/tavily-hikari/releases/latest/download/install-tvly-hikari.sh" | bash -s -- \
  --base-url "https://<your-host>" \
  --token "th-<id>-<secret>"
```

Then run `tvly-hikari search "query" --json`. Agent Skills are optional and installed separately:

```bash
npx skills add https://github.com/IvanLi-CN/tavily-hikari --global
```

The wrapper stores a Hikari token locally and routes official `tvly` commands to
`https://<your-host>/api/tavily`.

### Do I have to use ForwardAuth

No, but you do need at least one admin access strategy.

- ForwardAuth: recommended for production
- built-in admin login: good for self-hosted single-instance setups
- `DEV_OPEN_ADMIN=true`: only for local or temporary debugging

Without one of those strategies, the service may boot, but you will not have a stable way to
manage upstream keys.

### Do I have to enable Linux DO OAuth

No.

Linux DO OAuth is mainly for end-user sign-in and automatic Hikari token binding. If the service is
admin-operated only, or tokens are provisioned manually, you can leave it disabled.

### Where is the state stored

Long-lived state is stored in SQLite.

- Flag / Env: `--db-path` / `PROXY_DB_PATH`
- default container path: `/srv/app/data/tavily_proxy.db`

That database usually carries key state, token bindings, request audit data, and other operational
records, so persistence should be part of your deployment plan.

### What is the difference between the docs site and Storybook

- docs-site: operator and integrator documentation
- Storybook: UI review surface for page states, dialogs, tables, and admin flows

Use the docs site to deploy or integrate the product. Use Storybook to inspect the interface.

### Why does SerpApi sometimes return 429 right after another 429

SerpApi uses the same status code for two different conditions, and Hikari splits them apart:

- `429` with a `"run out of searches"` message means the upstream key's plan searches are used up.
  Hikari marks that key `exhausted`, and it only revives at the plan renewal date or after a manual
  recovery.
- any other `429` means hourly throughput throttling. Hikari applies a short transient backoff
  (honoring `Retry-After` between 30 and 3600 seconds), and the key recovers on its own.

If clients keep hitting `429` long after a backoff window, check the admin key health view instead
of retrying harder.

### Why does Exa return 402, and can I check an Exa balance

Exa bills from prepaid credits, so `402` with one of the budget tags (`NO_MORE_CREDITS`,
`API_KEY_BUDGET_EXCEEDED`, `TEAM_BUDGET_EXCEEDED`) means the key or the team budget is out. Hikari
marks the key `exhausted` until the next successful call or a manual restore; other `402` tags and
`403` pass through without penalizing the key.

Exa exposes no live remaining-balance API, so Hikari cannot sync a balance: admin `sync-usage`
returns a typed `409 unsupported`, and quota is tracked by local counting. Plan for one more
limit: the Exa team QPS ceiling (about 10 QPS for search/answer) is shared by every key in the
team, so pooling more keys does not raise it.

### Why does Brave return 422 for an invalid key, and how do monthly limits work

Brave reports an invalid key as `422` (not `401`):

- `422 SUBSCRIPTION_TOKEN_INVALID`, or a `VALIDATION` error pointing at `x-subscription-token`,
  means the key is invalid, and Hikari quarantines it.
- other `422 VALIDATION` errors are client parameter mistakes and pass through unchanged.

A Brave plan allows at most 10 keys upstream. Hikari harvests `X-RateLimit-*` response headers
passively: the month window (`w=2592000`) is paired with `Limit`/`Remaining`/`Reset`, and only
changed material is stored. A `402 QUOTA_LIMITED` (or bare `402`) exhausts the key for the
advertised window, while `429` is a short second-scale throttle (default 2s backoff, clamped to
1..=30s).

## Troubleshooting

### `/admin` does not open, or `/api/keys` returns 401

That usually means the admin access model is incomplete.

Check in this order:

1. If you use ForwardAuth, make sure:
   - `ADMIN_AUTH_FORWARD_ENABLED=true`
   - `FORWARD_AUTH_HEADER` and `FORWARD_AUTH_ADMIN_VALUE` are configured
   - your reverse proxy is actually injecting the matching admin header value
2. If you use the built-in admin login, make sure:
   - `ADMIN_AUTH_BUILTIN_ENABLED=true`
   - `ADMIN_AUTH_BUILTIN_PASSWORD_HASH` is configured
   - `ADMIN_AUTH_FORWARD_ENABLED=false` when ForwardAuth is not in use
3. If this is only local validation, you can temporarily use `DEV_OPEN_ADMIN=true`

One more important detail: the repository root `docker-compose.yml` only starts Hikari itself. It
does not automatically configure admin access.

### `/api/tavily/*` returns 401 Unauthorized

First verify that the token is being passed in a Hikari-compatible way:

- preferred: `Authorization: Bearer th-<id>-<secret>`
- for `POST /api/tavily/*` JSON requests, the token may also be sent as `api_key` in the body
- for `GET /api/tavily/usage` and `GET /api/tavily/research/:request_id`, use the
  `Authorization` header

If you are relying on `DEV_OPEN_ADMIN=true`, remember that this is only a local development
fallback, not a normal integration path.

### `/api/tavily/*` returns 429 Too Many Requests

That usually means either:

- the Hikari token hit its hourly request cap
- the token ran out of quota

Check the user dashboard or call `GET /api/tavily/usage` first. Then adjust quota, tags, or issue a
new token if needed.

### `/api/tavily/*` returns 502 Bad Gateway

The two most common causes are:

- there is no usable upstream Tavily key
- Hikari cannot reach the upstream endpoint

Check:

- whether the admin surface shows at least one active key
- whether `TAVILY_UPSTREAM` and related upstream settings are correct
- whether you are pointing at the intended mock, sandbox, or production upstream

### Keys, tokens, or request logs disappear after restart

This is usually a persistence issue, not an application logic issue.

Make sure:

- `PROXY_DB_PATH` points into a persistent volume or host-mounted directory
- upgrades and container re-creation still reuse the same SQLite file

If you use the default container path, persist at least `/srv/app/data`.

### `docker compose up -d` works, but there is no admin entry on the homepage

That is expected behavior, not a bug.

The repository root `docker-compose.yml` only starts Hikari. It does not automatically enable:

- ForwardAuth
- built-in admin login
- Linux DO OAuth

If you want to operate the service from that compose stack, add an admin access model on top. The
shortest path is documented in [Deployment & Anonymity](/deployment-anonymity).

### Local Storybook or docs links open the wrong thing or fail

Local cross-links assume both local review services are running:

- Storybook: `http://127.0.0.1:56006`
- docs-site: `http://127.0.0.1:56007`

If only one of them is running, local cross-links can fail. The final GitHub Pages deployment does
not depend on those local ports.

### How do I inspect which headers were stripped in high-anonymity mode

Look at the request-audit fields:

- `forwarded_headers`
- `dropped_headers`

For the deeper design background, read
[`docs/high-anonymity-proxy.md`](https://github.com/IvanLi-CN/tavily-hikari/blob/main/docs/high-anonymity-proxy.md).

## Related reading

- [Quick Start](/quick-start)
- [Configuration & Access](/configuration-access)
- [HTTP API Guide](/http-api-guide)
- [Deployment & Anonymity](/deployment-anonymity)
- [Storybook](/storybook.html)
