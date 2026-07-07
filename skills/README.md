# Tavily Hikari Agent Skills

Install these skills after installing the `tvly-hikari` CLI wrapper:

```bash
curl -fsSL https://github.com/IvanLi-CN/tavily-hikari/releases/latest/download/install-tvly-hikari.sh | bash -s -- \
  --base-url "https://<your-hikari-host>" \
  --token "th-<id>-<secret>"

npx skills add https://github.com/IvanLi-CN/tavily-hikari
```

The CLI wrapper injects:

- `TAVILY_API_BASE_URL=https://<your-hikari-host>/api/tavily`
- `TAVILY_API_KEY=th-<id>-<secret>`

The token is a Tavily Hikari access token, not an official Tavily API key. Traffic goes through
Hikari's `/api/tavily` facade so quota checks, audit logging, and upstream key-pool routing stay
inside the Hikari service.

Available skills:

- `tavily-hikari-cli`
- `tavily-hikari-search`
- `tavily-hikari-extract`
- `tavily-hikari-crawl`
- `tavily-hikari-map`
- `tavily-hikari-research`
- `tavily-hikari-best-practices`
