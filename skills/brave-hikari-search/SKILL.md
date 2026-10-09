---
name: brave-hikari-search
description: Search the web through Tavily Hikari with the Brave facade.
---

# Brave Hikari Search

Use this skill when the task needs independent-index web results through a Tavily Hikari
deployment.

## Workflow

1. Shape the query and optional filters (`country`, `search_lang`, `freshness`, `safesearch`).
2. Run:

   ```bash
   ORIGIN="https://<your-hikari-host>"
   curl -s "$ORIGIN/api/brave/res/v1/web/search?q=mcp+client+setup&count=10" \
     -H "X-Subscription-Token: th-<id>-<secret>"
   ```

3. Request specialized sections with `result_filter` (for example `faq` or `discussions`) and
   extra context with `extra_snippets=true`.
4. Read the native Brave JSON (`web.results`, `query`) and cite result URLs.

## Boundaries

- Web, news, and LLM context only. Images, videos, places, suggest, spellcheck, summarizer, and
  Answers endpoints have no Hikari routes.
- No URL extraction, crawl, or map surface.
- Result caps: `count` 1-20, `offset` 0-9.

## Hikari Notes

- Base URL: `https://<your-hikari-host>/api/brave` mirrors `https://api.search.brave.com`.
- Auth slots: `X-Subscription-Token: th-...` (native header) or `Authorization: Bearer th-...`.
- Hikari bills 1 credit per successful web search; failures and quota rejections bill 0.
- MCP clients can use `https://<your-hikari-host>/brave/mcp`; tool `brave_web_search` bills 1
  credit; `initialize` and `tools/list` are free.
