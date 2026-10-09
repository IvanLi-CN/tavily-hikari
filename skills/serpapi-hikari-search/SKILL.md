---
name: serpapi-hikari-search
description: Search SERP results through Tavily Hikari with the SerpApi facade.
---

# SerpApi Hikari Search

Use this skill when the task needs SERP-native search results through a Tavily Hikari deployment.

## Workflow

1. Shape a focused query and pick the SerpApi engine. `engine=google` is the tested default; the
   parameter passes through verbatim, so other SerpApi engines keep their native semantics.
2. Run a native GET request with the Hikari token in the SerpApi `api_key` slot:

   ```bash
   ORIGIN="https://<your-hikari-host>"
   curl -s "$ORIGIN/api/serpapi/search?q=coffee&engine=google&api_key=th-<id>-<secret>"
   ```

3. Or use the POST convenience variant: every JSON body field becomes an upstream query parameter.

   ```bash
   curl -s -X POST "$ORIGIN/api/serpapi/search" \
     -H "Authorization: Bearer th-<id>-<secret>" \
     -H "Content-Type: application/json" \
     -d '{"q": "coffee", "engine": "google", "num": 5}'
   ```

4. Read the native SerpApi JSON (`organic_results`, `search_metadata`, `search_parameters`) and
   cite result links when the answer depends on external facts.

## Boundaries

- Synchronous JSON searches. Async mode still needs the SerpApi archive endpoint to collect the
  results, and that endpoint has no Hikari route.
- The locations endpoint is absent, and SerpApi's HTML output mode is not part of this facade;
  work from the native JSON payload (`output=json` or the default).
- The only SerpApi REST facade route is `/api/serpapi/search`.

## Hikari Notes

- Base URL: `https://<your-hikari-host>/api/serpapi` mirrors `https://serpapi.com`.
- Auth slots: `api_key=th-...` (native query slot) or `Authorization: Bearer th-...`.
- POST bodies drop `api_key` fields and never persist them; explicit URL query parameters win over
  same-name body fields.
- Hikari rotates the pooled SerpApi key and bills 1 credit per successful search. Failures, quota
  rejections, and invalid requests bill 0.
- MCP clients can use `https://<your-hikari-host>/serpapi/mcp` with bearer auth. Tools: `search`,
  `search_table`, `search_dashboard` (1 credit per successful call); `initialize`, `tools/list`,
  and other control-plane methods are free.
