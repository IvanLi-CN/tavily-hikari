# Configuration & Access

## Core runtime flags

| Flag / Env                        | Purpose                          |
| --------------------------------- | -------------------------------- |
| `--upstream` / `TAVILY_UPSTREAM`  | Tavily upstream MCP endpoint     |
| `--bind` / `PROXY_BIND`           | Listen address                   |
| `--port` / `PROXY_PORT`           | Listen port                      |
| `--db-path` / `PROXY_DB_PATH`     | SQLite file path                 |
| `--static-dir` / `WEB_STATIC_DIR` | Override static assets directory |
| `--keys` / `TAVILY_API_KEYS`      | Optional bootstrap import helper |

Even though `TAVILY_API_KEYS` exists, production operation should prefer the admin API or web UI
for key lifecycle management.

## Admin access models

### ForwardAuth

Recommended for production or any zero-trust topology.

```bash
export ADMIN_AUTH_FORWARD_ENABLED=true
export FORWARD_AUTH_HEADER=Remote-Email
export FORWARD_AUTH_ADMIN_VALUE=admin@example.com
export FORWARD_AUTH_NICKNAME_HEADER=Remote-Name
```

The authenticated header value controls whether a caller may access `/api/keys/*` and `/admin`.

### Built-in admin login

Useful for self-hosted or small deployments when a separate ForwardAuth gateway is unavailable.

```bash
export ADMIN_AUTH_BUILTIN_ENABLED=true
export ADMIN_AUTH_BUILTIN_PASSWORD_HASH='<phc-string>'
export ADMIN_AUTH_FORWARD_ENABLED=false
```

This enables an HttpOnly cookie session and shows an **Admin Login** action on the public home
surface.

## End-user Linux DO OAuth

```bash
export LINUXDO_OAUTH_ENABLED=true
export LINUXDO_OAUTH_CLIENT_ID='<client-id>'
export LINUXDO_OAUTH_CLIENT_SECRET='<client-secret>'
export LINUXDO_OAUTH_REDIRECT_URL='https://<your-host>/auth/linuxdo/callback'
```

Behavior summary:

- first successful login creates and binds one Hikari access token
- later logins reuse the same binding
- new users start with zero built-in base quota unless an admin grants quota via tags or manual
  settings

## Access tokens for HTTP clients

Tavily Hikari issues access tokens in the form `th-<id>-<secret>`. These tokens are used by end
users and HTTP clients instead of exposing upstream Tavily keys directly.

- Admin APIs use admin auth (ForwardAuth or built-in admin cookie)
- `/api/tavily/*` uses the Hikari access token
- `/mcp` requests are proxied upstream after Hikari routing and anonymity handling

## Related reading

- [HTTP API Guide](/http-api-guide)
- [Deployment & Anonymity](/deployment-anonymity)
- [Storybook Guide](/storybook-guide.html)
