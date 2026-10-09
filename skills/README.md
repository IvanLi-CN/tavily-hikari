# Tavily Hikari Agent Skills

Install these skills after installing the `tvly-hikari` CLI wrapper:

```bash
curl -fsSL https://github.com/IvanLi-CN/tavily-hikari/releases/latest/download/install-tvly-hikari.sh | bash -s -- \
  --base-url "https://<your-hikari-host>" \
  --token "th-<id>-<secret>"

npx skills add https://github.com/IvanLi-CN/tavily-hikari --global
```

This installs the package globally in the active agent client's standard Skills location.

The CLI wrapper injects:

- `TAVILY_API_BASE_URL=https://<your-hikari-host>/api/tavily`
- `TAVILY_API_KEY=th-<id>-<secret>`

The token is a Tavily Hikari access token, not an official Tavily API key. Traffic goes through
Hikari's `/api/tavily` facade so quota checks, audit logging, and upstream key-pool routing stay
inside the Hikari service.

The provider facade skills talk to Hikari's native HTTP and MCP endpoints directly and need only
your Hikari origin plus a `th-` token.

Available skills:

- `tavily-hikari-cli`
- `tavily-hikari-search`
- `tavily-hikari-extract`
- `tavily-hikari-crawl`
- `tavily-hikari-map`
- `tavily-hikari-research`
- `tavily-hikari-best-practices`
- `serpapi-hikari-search`
- `exa-hikari-search`
- `exa-hikari-contents`
- `exa-hikari-answer`
- `brave-hikari-search`
- `brave-hikari-news`
- `brave-hikari-llm-context`

## Provider Endpoints and Auth Slots

Every facade also accepts `Authorization: Bearer th-...` as the universal slot, in addition to
the provider-native slot below. Replace `<origin>` with your Hikari origin, for example
`https://<your-hikari-host>`.

| Provider | REST base              | REST auth slot                 | MCP endpoint           | MCP native slot                           |
| -------- | ---------------------- | ------------------------------ | ---------------------- | ----------------------------------------- |
| Tavily   | `<origin>/api/tavily`  | `Authorization: Bearer th-...` | `<origin>/mcp`         | `Bearer th-...`                           |
| SerpApi  | `<origin>/api/serpapi` | `api_key=th-...` (query)       | `<origin>/serpapi/mcp` | `Bearer th-...`                           |
| Exa      | `<origin>/api/exa`     | `x-api-key: th-...`            | `<origin>/exa/mcp`     | `x-api-key: th-...` or `exaApiKey=th-...` |
| Brave    | `<origin>/api/brave`   | `X-Subscription-Token: th-...` | `<origin>/brave/mcp`   | `X-Subscription-Token: th-...`            |

- Tavily MCP clients use the existing `/mcp` endpoint; `TAVILY_API_BASE_URL` and `TAVILY_API_KEY`
  stay owned by the `tvly-hikari` wrapper.
- SerpApi, Exa, and Brave MCP endpoints are dedicated Hikari ingresses; Exa has no hosted
  passthrough, so `/exa/mcp` is served entirely by the local facade.
- See `tavily-hikari-best-practices` for provider selection, credit costs, and exhaustion
  semantics.
