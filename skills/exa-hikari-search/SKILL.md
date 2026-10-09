---
name: exa-hikari-search
description: Run semantic web search through Tavily Hikari with the Exa facade.
---

# Exa Hikari Search

Use this skill when the task needs neural or keyword web search through a Tavily Hikari deployment.

## Workflow

1. Write a query that states the meaning you want, not only keywords; Exa ranks by semantic
   similarity by default.
2. Run:

   ```bash
   ORIGIN="https://<your-hikari-host>"
   curl -s -X POST "$ORIGIN/api/exa/search" \
     -H "x-api-key: th-<id>-<secret>" \
     -H "Content-Type: application/json" \
     -d '{"query": "agent memory architectures", "numResults": 5}'
   ```

3. Optionally request page text alongside the results with `"contents": {"text": true}`.
4. Read the native Exa JSON (`results`, `costDollars`, `requestId`) and cite result URLs.

## Boundaries

- No streaming. Any truthy `stream` value is rejected locally with a typed 400 naming the
  boundary (`streaming not supported; omit stream or set false`); the facade never returns SSE.
- No `/findSimilar`, `/agent/*`, `/monitors`, or `/batches` routes; only `/api/exa/search` exists.

## Hikari Notes

- Base URL: `https://<your-hikari-host>/api/exa` mirrors `https://api.exa.ai`.
- Auth slots: `x-api-key: th-...` (native header slot) or `Authorization: Bearer th-...`.
- Hikari bills 1 credit per successful search; errors, stream rejections, and quota rejections
  bill 0.
- MCP clients can use `https://<your-hikari-host>/exa/mcp` (local facade only, no hosted Exa
  passthrough). Search tool: `web_search_exa` (1 credit); `initialize` and `tools/list` are free.
