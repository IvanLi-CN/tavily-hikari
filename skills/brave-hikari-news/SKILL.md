---
name: brave-hikari-news
description: Search news through Tavily Hikari with the Brave facade.
---

# Brave Hikari News

Use this skill when the task needs recent news results through a Tavily Hikari deployment.

## Workflow

1. Shape the query with a recency window (`freshness=pd`, `pw`, `pm`, or a date range).
2. Run:

   ```bash
   ORIGIN="https://<your-hikari-host>"
   curl -s "$ORIGIN/api/brave/res/v1/news/search?q=ai+regulation&freshness=pd&count=20" \
     -H "X-Subscription-Token: th-<id>-<secret>"
   ```

3. Read the native Brave JSON (`results`) and cite each source with its publication URL.

## Boundaries

- News search only; web search uses the separate web endpoint.
- Result caps: `count` 1-50, `offset` 0-9.
- No summarizer, no URL extraction, and no crawl or map surface.

## Hikari Notes

- Base URL: `https://<your-hikari-host>/api/brave` mirrors `https://api.search.brave.com`.
- Auth slots: `X-Subscription-Token: th-...` or `Authorization: Bearer th-...`.
- Hikari bills 1 credit per successful news search.
- MCP clients can use `https://<your-hikari-host>/brave/mcp`; tool `brave_news_search` bills 1
  credit.
