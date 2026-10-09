# HTTP API Guide

## What this page is for

Use this page when your client speaks Tavily's HTTP API directly instead of MCP.

Typical cases:

- Cherry Studio and similar clients that only need a Base URL plus API key
- scripts, webhooks, or backend services that call Tavily over JSON
- deployments where end users must never see the real upstream Tavily key

If you are integrating an MCP client, use `/mcp` instead of `/api/tavily/*`.

## Currently available Tavily facade endpoints

| Method | Path                               | Notes                           | Auth         |
| ------ | ---------------------------------- | ------------------------------- | ------------ |
| `POST` | `/api/tavily/search`               | Proxy Tavily `/search`          | Hikari token |
| `POST` | `/api/tavily/extract`              | Proxy Tavily `/extract`         | Hikari token |
| `POST` | `/api/tavily/crawl`                | Proxy Tavily `/crawl`           | Hikari token |
| `POST` | `/api/tavily/map`                  | Proxy Tavily `/map`             | Hikari token |
| `POST` | `/api/tavily/research`             | Proxy Tavily `/research`        | Hikari token |
| `GET`  | `/api/tavily/research/:request_id` | Fetch a research result         | Hikari token |
| `GET`  | `/api/tavily/usage`                | Daily and monthly usage summary | Hikari token |

## Authentication

Preferred form:

```http
Authorization: Bearer th-<id>-<secret>
```

For `POST /api/tavily/*` JSON requests, Hikari also accepts the token in the request body:

```json
{
  "api_key": "th-<id>-<secret>",
  "query": "rust async runtime"
}
```

Additional rules:

- if both header and body contain a token, `Authorization: Bearer ...` wins
- GET endpoints such as `/api/tavily/research/:request_id` and `/api/tavily/usage` should use the
  `Authorization` header, not a request body
- local `DEV_OPEN_ADMIN=true` mode allows tokenless fallback for debugging; do not rely on that in
  production

## Search example

```bash
curl -X POST http://127.0.0.1:58087/api/tavily/search \
  -H "Authorization: Bearer th-<id>-<secret>" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "rust async runtime",
    "topic": "general",
    "search_depth": "basic",
    "max_results": 3
  }'
```

If the client can only send the token in the JSON body:

```bash
curl -X POST http://127.0.0.1:58087/api/tavily/search \
  -H "Content-Type: application/json" \
  -d '{
    "api_key": "th-<id>-<secret>",
    "query": "rust async runtime",
    "topic": "general"
  }'
```

## Other Tavily HTTP endpoints

Besides `/search`, the proxy currently supports:

- `/api/tavily/extract`
- `/api/tavily/crawl`
- `/api/tavily/map`
- `/api/tavily/research`

They follow the same contract:

- client-facing JSON stays as close to Tavily HTTP conventions as practical
- Hikari strips the client token from `api_key` before the upstream call
- Hikari injects a pooled real Tavily key internally
- quota checks, audit logging, and key selection all happen inside Hikari

Research result retrieval example:

```bash
curl -H "Authorization: Bearer th-<id>-<secret>" \
  http://127.0.0.1:58087/api/tavily/research/<request_id>
```

## SerpApi, Exa, and Brave facades

Besides `/api/tavily/*`, Hikari exposes native REST facades for three more search providers. Each
facade accepts the same Hikari token (`th-<id>-<secret>`) in its provider-native auth slot, strips
it before the upstream call, and injects a pooled upstream key internally. Credits for all four
providers settle from the same shared wallet.

### SerpApi

| Method | Path                  | Notes                                 | Auth                                        |
| ------ | --------------------- | ------------------------------------- | ------------------------------------------- |
| `GET`  | `/api/serpapi/search` | SerpApi `/search` (query passthrough) | `Authorization: Bearer` or `api_key=th-...` |
| `POST` | `/api/serpapi/search` | JSON body mapped to query parameters  | `Authorization: Bearer` or `api_key=th-...` |

MCP endpoint: `https://<your-host>/serpapi/mcp` (hosted passthrough, `Authorization: Bearer th-...`).

```bash
curl "https://<your-host>/api/serpapi/search?engine=google&q=rust+async+runtime&api_key=th-<id>-<secret>"
```

For POST requests the token must ride the `Authorization` header or the query string; a JSON
`api_key` body field is discarded. `search` costs 1 credit. Boundaries: synchronous search JSON
only; async searches, search-archive, `locations`, and `output=html` are not proxied.

### Exa

| Method | Path                | Notes                | Auth                                           |
| ------ | ------------------- | -------------------- | ---------------------------------------------- |
| `POST` | `/api/exa/search`   | Exa `/search`        | `x-api-key: th-...` or `Authorization: Bearer` |
| `POST` | `/api/exa/contents` | URL content extract  | `x-api-key: th-...` or `Authorization: Bearer` |
| `POST` | `/api/exa/answer`   | Non-streaming answer | `x-api-key: th-...` or `Authorization: Bearer` |

MCP endpoint: `https://<your-host>/exa/mcp` (local facade; `x-api-key: th-...`,
`exaApiKey=th-...` query, or `Authorization: Bearer th-...`).

```bash
curl -X POST "https://<your-host>/api/exa/search" \
  -H "x-api-key: th-<id>-<secret>" \
  -H "Content-Type: application/json" \
  -d '{"query": "rust async runtime", "numResults": 3}'
```

Credits: `search` and `contents` cost 1, `answer` costs 2. Boundaries: streaming is rejected
locally with a typed `400` (`streaming not supported; omit stream or set false`). `findSimilar`,
the agent/monitors/batches/websets surfaces, `chat/completions` compatibility, and hosted MCP
passthrough are not proxied.

### Brave

| Method | Path                            | Notes                  | Auth                                     |
| ------ | ------------------------------- | ---------------------- | ---------------------------------------- |
| `GET`  | `/api/brave/res/v1/web/search`  | Web search             | `X-Subscription-Token: th-...` or Bearer |
| `GET`  | `/api/brave/res/v1/news/search` | News search            | `X-Subscription-Token: th-...` or Bearer |
| `GET`  | `/api/brave/res/v1/llm/context` | LLM grounding snippets | `X-Subscription-Token: th-...` or Bearer |

MCP endpoint: `https://<your-host>/brave/mcp` (local facade; `X-Subscription-Token: th-...` or
`Authorization: Bearer th-...`).

```bash
curl "https://<your-host>/api/brave/res/v1/web/search?q=rust+async+runtime" \
  -H "X-Subscription-Token: th-<id>-<secret>"
```

Credits: `web` and `news` cost 1, `llm_context` costs 2. Boundaries: only web search, news search,
and LLM Context are proxied; URL extraction/crawl, images, videos, local/place, suggest,
spellcheck, summarizer, and Answers are not.

## Cherry Studio setup

1. Create a Tavily Hikari access token from the user dashboard.
2. In Cherry Studio, choose the **Tavily (API key)** provider.
3. Set the API URL to `https://<your-host>/api/tavily`.
4. Use the Hikari token `th-<id>-<secret>` as the API key.

Do **not** paste the official Tavily API key into Cherry Studio when Hikari is in front of it.

For local development, the API URL is usually:

`http://127.0.0.1:58087/api/tavily`

## CLI + Agent Skills setup

If you want the official Tavily CLI workflow but routed through Hikari, install `tvly-hikari`:

```bash
curl -fsSL "https://github.com/IvanLi-CN/tavily-hikari/releases/latest/download/install-tvly-hikari.sh" | bash -s -- \
  --base-url "https://<your-host>" \
  --token "th-<id>-<secret>"
```

The wrapper passes commands to the official `tvly` executable while setting:

```bash
TAVILY_API_BASE_URL=https://<your-host>/api/tavily
TAVILY_API_KEY=th-<id>-<secret>
```

Optional Agent Skills:

```bash
npx skills add https://github.com/IvanLi-CN/tavily-hikari --global
```

`TAVILY_API_KEY` is intentionally the Hikari token in this downstream context. Hikari swaps in the
real upstream Tavily key internally after quota and audit checks.

## Common responses and errors

- `401 Unauthorized`
  - token is missing
  - token is invalid
  - token is disabled
- `429 Too Many Requests`
  - hourly request count limit is exhausted
  - credits quota for the token is exhausted
- `400 Bad Request`
  - request body is not valid JSON
  - a field such as `max_results` is obviously invalid
- `502 Bad Gateway`
  - Hikari cannot reach the upstream Tavily endpoint
  - there is no available upstream key

On success, Hikari tries to return the upstream Tavily body directly instead of wrapping it in an
extra envelope.

## `/mcp` versus `/api/tavily/*`

- `/mcp`
  - for Codex CLI, Cursor, Claude Desktop, and other MCP clients
  - speaks standard MCP-over-HTTP transport
- `/api/tavily/*`
  - for Cherry Studio, scripts, and plain HTTP integrations
  - speaks Tavily-style JSON endpoints

Both paths still share:

- Hikari access tokens
- the Tavily key pool
- quota enforcement
- request auditing

## Related endpoints

| Method | Path                  | Notes                          |
| ------ | --------------------- | ------------------------------ |
| `GET`  | `/health`             | Liveness probe                 |
| `GET`  | `/api/summary`        | Public summary metrics         |
| `GET`  | `/api/user/token`     | Resolve the current user token |
| `GET`  | `/api/user/dashboard` | User dashboard summary         |
| `POST` | `/api/user/logout`    | End-user logout                |
| `GET`  | `/api/keys`           | Admin list of upstream keys    |
| `POST` | `/api/keys`           | Admin add or restore a key     |

Those admin endpoints use admin authentication, not Hikari token auth.

## When to use Storybook instead

If you are reviewing operator workflows or dashboard states rather than integrating an API client,
open [Storybook](/storybook.html) instead of this page.

If the integration fails with `401`, `429`, `502`, or token-passing confusion, continue with
[FAQ & Troubleshooting](/faq).
