---
name: tavily-hikari-best-practices
description: Guide agents to use Tavily Hikari and its search providers safely.
---

# Tavily Hikari Best Practices

Use this skill when deciding how to call Tavily Hikari from an agent workflow.

## Rules

- Use `tvly-hikari ... --json` for agent-readable output.
- Use Hikari tokens beginning with `th-`; never paste official Tavily API keys into downstream
  agents or user machines.
- Configure Hikari once:

  ```bash
  tvly-hikari configure --base-url https://<your-hikari-host> --token th-<id>-<secret>
  ```

- Treat `/mcp` and `/api/tavily/*` as different integration surfaces:
  - MCP clients use `https://<your-hikari-host>/mcp` with bearer auth.
  - CLI and HTTP-style clients use `https://<your-hikari-host>/api/tavily`.
- Keep searches narrow, save large outputs with `-o` or `--output-dir`, and cite source URLs when
  answering factual questions.
- Do not test against Tavily production upstream unless the operator explicitly approves it; use
  local or sandbox Hikari endpoints for smoke tests.

## Cross-Provider Comparison

Hikari fronts four search providers. Each keeps its own native request shape, key pool, and
exhaustion model, so pick the provider that matches the task before writing queries.

### When to use which provider

- **Tavily** (via `tvly-hikari` or `/api/tavily`) for agent workflows that need extract, crawl,
  map, or research alongside search.
- **SerpApi** (`/api/serpapi/search`) for SERP-native Google-engine results, including structured
  result blocks such as `organic_results`.
- **Exa** (`/api/exa/*`) for neural and semantic retrieval, page contents by URL or document id,
  and synthesized answers with citations.
- **Brave** (`/api/brave/*`) for an independent index with a privacy posture, plus the
  `/llm/context` endpoint for token-bounded context blocks.

### Credit costs

One Hikari token spends from a single credit wallet shared by every provider, so a Brave search
can consume credits a Tavily call would otherwise have used.

| Provider | REST facade                         | MCP tools                                                   | Credits                                                        |
| -------- | ----------------------------------- | ----------------------------------------------------------- | -------------------------------------------------------------- |
| Tavily   | `/api/tavily/*`                     | `/mcp` (`tavily-search`, `tavily-extract`)                  | Upstream `usage.credits` (search 1-2); research est. 40/50/100 |
| SerpApi  | `GET/POST /api/serpapi/search`      | `/serpapi/mcp` `search`, `search_table`, `search_dashboard` | 1 per successful call                                          |
| Exa      | `POST /api/exa/search`              | `/exa/mcp` `web_search_exa`                                 | 1                                                              |
| Exa      | `POST /api/exa/contents`            | `/exa/mcp` `web_fetch_exa`                                  | 1                                                              |
| Exa      | `POST /api/exa/answer`              | `/exa/mcp` `exa_answer`                                     | 2                                                              |
| Brave    | `GET /api/brave/res/v1/web/search`  | `/brave/mcp` `brave_web_search`                             | 1                                                              |
| Brave    | `GET /api/brave/res/v1/news/search` | `/brave/mcp` `brave_news_search`                            | 1                                                              |
| Brave    | `GET /api/brave/res/v1/llm/context` | `/brave/mcp` `brave_llm_context`                            | 2                                                              |

Provider failures, local validation rejections, and quota-blocked requests bill zero. When the
token wallet runs dry, Hikari answers a local `429 quota_exhausted` before any upstream call,
regardless of provider.

### Exhaustion and error semantics

- **Tavily**: a `432` marks the upstream key `exhausted` until the next UTC month; low-remaining
  keys stay out of normal monthly pools.
- **SerpApi**: dual `429` semantics. A run-out `429` whose body is
  `{"error":"Your account has run out of searches."}` marks the key `exhausted` until its plan
  renewal date. An hourly-throughput `429` arms a transient backoff (default 3600 s, honoring
  `Retry-After` in 30-3600 s) and keeps the key active.
- **Exa**: `402` budget tags (`NO_MORE_CREDITS`, `TEAM_BUDGET_EXCEEDED`) mark the key `exhausted`;
  there is no calendar reset, and the next successful call restores it. `429` parses `Retry-After`
  into a transient per-key backoff. `401` quarantines the key as `invalid_api_key`.
- **Brave**: `422 SUBSCRIPTION_TOKEN_INVALID` means the key itself is invalid, so it is
  quarantined rather than exhausted. `402 QUOTA_LIMITED` marks the key `exhausted` for the
  advertised month window (`X-RateLimit-Reset`, harvested passively from response headers). `429`
  arms a second-scale backoff (2 s default) and keeps the key active.

## Quick Smoke

```bash
tvly-hikari doctor
tvly-hikari search "Tavily Hikari smoke test" --json
```
