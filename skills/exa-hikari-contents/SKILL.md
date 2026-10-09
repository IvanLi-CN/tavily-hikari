---
name: exa-hikari-contents
description: Fetch page contents through Tavily Hikari with the Exa facade.
---

# Exa Hikari Contents

Use this skill when the task needs clean page contents for known URLs or Exa document ids.

## Workflow

1. Collect the URLs, or the Exa document `ids` from an earlier search response.
2. Run with URLs:

   ```bash
   ORIGIN="https://<your-hikari-host>"
   curl -s -X POST "$ORIGIN/api/exa/contents" \
     -H "x-api-key: th-<id>-<secret>" \
     -H "Content-Type: application/json" \
     -d '{"urls": ["https://example.com/1"]}'
   ```

   Or with document ids:

   ```bash
   curl -s -X POST "$ORIGIN/api/exa/contents" \
     -H "x-api-key: th-<id>-<secret>" \
     -H "Content-Type: application/json" \
     -d '{"ids": ["<exa-document-id>"]}'
   ```

3. Read the native `results` / `statuses` envelope and quote only the passages you need.

## Boundaries

- Provide exactly one of `urls` or `ids` (XOR). Both sides or neither side is an invalid request:
  the Exa MCP facade rejects it locally, and the REST facade forwards it to Exa's native 4xx.
- No streaming; truthy `stream` values get the typed 400 boundary response.
- No crawl, map, or find-similar routes.

## Hikari Notes

- Base URL: `https://<your-hikari-host>/api/exa` mirrors `https://api.exa.ai`.
- Auth slots: `x-api-key: th-...` or `Authorization: Bearer th-...`.
- Hikari bills 1 credit per successful contents call; failures bill 0.
- MCP clients can use `https://<your-hikari-host>/exa/mcp`; tool `web_fetch_exa` accepts
  `urls[]` (1-100 items) or `ids[]` and bills 1 credit.
