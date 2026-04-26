# Configuration & Access

This page is not just an environment-variable list. It answers three practical questions:

1. which settings are the minimum for this deployment
2. how admins are supposed to get in
3. which token downstream clients should actually use

## Start by choosing an access model

Tavily Hikari has two different access layers:

- admin access: who may enter `/admin` and call admin endpoints such as `/api/keys`
- user / client access: who may call `/mcp` or `/api/tavily/*`

Recommended defaults:

- local development: `DEV_OPEN_ADMIN=true`
- self-hosted single instance: built-in admin login
- production gateway: ForwardAuth
- end-user sign-in: Linux DO OAuth only when needed

## Core runtime settings

No matter which access model you choose, these settings are the main runtime contract:

| Flag / Env                        | Usually matters | Purpose                         |
| --------------------------------- | --------------- | ------------------------------- |
| `--upstream` / `TAVILY_UPSTREAM`  | yes             | Tavily MCP upstream URL         |
| `TAVILY_USAGE_BASE`               | yes             | Tavily HTTP / usage base URL    |
| `--bind` / `PROXY_BIND`           | yes             | listen address                  |
| `--port` / `PROXY_PORT`           | yes             | listen port                     |
| `--db-path` / `PROXY_DB_PATH`     | yes             | SQLite database path            |
| `LOW_QUOTA_DEPLETION_THRESHOLD`   | optional        | low-balance 432 key threshold   |
| `--static-dir` / `WEB_STATIC_DIR` | depends         | frontend static asset directory |
| `--keys` / `TAVILY_API_KEYS`      | optional        | one-time key bootstrap helper   |

Notes:

- `TAVILY_UPSTREAM` has a default and usually does not need to be overridden in local development,
  but you should override it when using a mock, sandbox, or custom upstream.
- `TAVILY_USAGE_BASE` defaults to `https://api.tavily.com` and affects usage / quota sync flows.
- `TAVILY_UPSTREAM` is treated as the full MCP endpoint. If your reverse proxy preserves a path
  prefix, include the final `/mcp` path in the configured URL.
- `TAVILY_USAGE_BASE` may also include a path prefix. Hikari appends `/search`, `/extract`,
  `/crawl`, `/map`, `/research`, `/research/{id}`, and `/usage` under that prefix.
- `LOW_QUOTA_DEPLETION_THRESHOLD` defaults to `15`. When an upstream key returns 432 and its
  latest known remaining credits are at or below this value, Hikari keeps it out of normal key
  pools for the current UTC month while still allowing it as a final fallback.
- `WEB_STATIC_DIR` is optional. If omitted, the app will try to use `web/dist` when that directory
  exists.
- `TAVILY_API_KEYS` is convenient for bootstrapping, but long-term key lifecycle should be managed
  through the admin UI or admin API.

## Minimum config by deployment shape

### Local development

This is the shortest path:

```bash
export DEV_OPEN_ADMIN=true
export PROXY_BIND=127.0.0.1
export PROXY_PORT=58087
```

That gives you:

- direct access to admin endpoints
- a quick path to inject the first upstream Tavily key
- local fallback behavior for `/mcp` and `/api/tavily/*` while validating the app

Use this only for local development.

### Self-hosted single instance

If there is no dedicated gateway, the simplest stable setup is the built-in admin login:

```bash
export ADMIN_AUTH_BUILTIN_ENABLED=true
export ADMIN_AUTH_BUILTIN_PASSWORD_HASH='<phc-string>'
export ADMIN_AUTH_FORWARD_ENABLED=false
```

This is the minimum reliable self-hosted setup. The admin signs in through the browser, receives an
HttpOnly cookie session, and then uses `/admin` and the admin API from there.

### Production gateway / zero-trust edge

If you already have a trusted reverse proxy or identity gateway, use ForwardAuth:

```bash
export ADMIN_AUTH_FORWARD_ENABLED=true
export FORWARD_AUTH_HEADER=Remote-Email
export FORWARD_AUTH_ADMIN_VALUE=admin@example.com
export FORWARD_AUTH_NICKNAME_HEADER=Remote-Name
```

In that model, admin status is derived from the trusted identity header, not from a local cookie.

## Admin access models

### ForwardAuth

This is the recommended production model.

There are only three things that really matter:

1. which header identifies the caller: `FORWARD_AUTH_HEADER`
2. which value is treated as admin: `FORWARD_AUTH_ADMIN_VALUE`
3. whether the reverse proxy actually injects that value into the request

The most common misunderstanding is this:

- `ADMIN_AUTH_FORWARD_ENABLED=true` is the default
- but if `FORWARD_AUTH_HEADER` is not configured, that does not magically create admin access

In other words, “the switch is on” does not mean “ForwardAuth integration is complete”.

### Built-in admin login

The built-in admin login is a good fit for:

- self-hosted single-instance deployments
- internal deployments
- setups that do not yet have a dedicated ForwardAuth or SSO layer

Required rule:

- once `ADMIN_AUTH_BUILTIN_ENABLED=true`
- you must provide either `ADMIN_AUTH_BUILTIN_PASSWORD` or `ADMIN_AUTH_BUILTIN_PASSWORD_HASH`

Recommended rule:

- prefer `ADMIN_AUTH_BUILTIN_PASSWORD_HASH`
- avoid keeping `ADMIN_AUTH_BUILTIN_PASSWORD` around long-term

Generate a hash like this:

```bash
echo -n 'change-me' | cargo run --quiet --bin admin_password_hash
```

### `DEV_OPEN_ADMIN`

This is the development shortcut:

```bash
export DEV_OPEN_ADMIN=true
```

It does two things:

- opens admin access for local validation
- allows local fallback behavior for `/mcp` and `/api/tavily/*` when you are still wiring up the
  real auth path

Do not treat it as a production-grade access model.

## Linux DO OAuth

Linux DO OAuth is only for end-user sign-in. It is not an admin-auth mechanism.

Enable it when you want end users to log in through the web UI and automatically reuse their bound
Hikari token:

```bash
export LINUXDO_OAUTH_ENABLED=true
export LINUXDO_OAUTH_CLIENT_ID='<client-id>'
export LINUXDO_OAUTH_CLIENT_SECRET='<client-secret>'
export LINUXDO_OAUTH_REDIRECT_URL='https://<your-host>/auth/linuxdo/callback'
```

Those three values are required together:

- `LINUXDO_OAUTH_CLIENT_ID`
- `LINUXDO_OAUTH_CLIENT_SECRET`
- `LINUXDO_OAUTH_REDIRECT_URL`

Other OAuth settings already have defaults and only matter when you need to override provider
endpoints:

- `LINUXDO_OAUTH_AUTHORIZE_URL`
- `LINUXDO_OAUTH_TOKEN_URL`
- `LINUXDO_OAUTH_USERINFO_URL`
- `LINUXDO_OAUTH_SCOPE`

Session tuning is also available:

- `USER_SESSION_MAX_AGE_SECS`
- `OAUTH_LOGIN_STATE_TTL_SECS`

If Linux DO OAuth stays disabled, admins can still issue tokens manually for downstream clients.

## Which token should clients use

This distinction matters:

- admin endpoints use the admin access model, not Hikari tokens
- `/api/tavily/*` uses Hikari tokens
- `/mcp` also uses Hikari tokens

The Hikari token format is:

```text
th-<id>-<secret>
```

That is the token you should hand to:

- Cherry Studio
- scripts
- custom backends
- MCP clients

Do not hand raw Tavily API keys to those downstream consumers.

## Less common but supported advanced settings

Most deployments do not need these immediately, but they are part of the supported runtime
contract:

| Flag / Env                     | Purpose                                                                     |
| ------------------------------ | --------------------------------------------------------------------------- |
| `XRAY_BINARY`                  | Xray binary path for share-link based forward proxies                       |
| `XRAY_RUNTIME_DIR`             | Xray runtime directory                                                      |
| `API_KEY_IP_GEO_ORIGIN`        | origin for API key registration IP geo lookup                               |
| `ADMIN_MODE_NAME`              | override the displayed admin-mode name                                      |
| `FORWARD_AUTH_NICKNAME_HEADER` | display nickname passed through the gateway                                 |
| `ADMIN_AUTH_BUILTIN_PASSWORD`  | legacy plaintext built-in admin password, still supported but not preferred |

If your goal is local development, straightforward self-hosting, or a standard gateway deployment,
you usually do not need these first.

## One-page decision summary

If you only want the shortest correct answer:

- local development: `DEV_OPEN_ADMIN=true`
- single-instance self-hosting:
  `ADMIN_AUTH_BUILTIN_ENABLED=true` + `ADMIN_AUTH_BUILTIN_PASSWORD_HASH`
- gateway integration:
  `ADMIN_AUTH_FORWARD_ENABLED=true` + `FORWARD_AUTH_HEADER` + `FORWARD_AUTH_ADMIN_VALUE`
- end-user web login: additionally enable `LINUXDO_OAUTH_ENABLED=true`
- downstream client access: always use `th-<id>-<secret>`, never raw Tavily API keys

## Related reading

- [Quick Start](/quick-start)
- [HTTP API Guide](/http-api-guide)
- [Deployment & Anonymity](/deployment-anonymity)
- [FAQ & Troubleshooting](/faq)
- [Storybook](/storybook.html)
