---
name: tavily-hikari-map
description: Discover site URLs through Tavily Hikari with tvly-hikari.
---

# Tavily Hikari Map

Use this skill when a task needs URL discovery or site mapping through Tavily Hikari.

## Workflow

1. Start from a canonical site or documentation root.
2. Run:

   ```bash
   tvly-hikari map https://example.com/docs --json
   ```

3. Save output when the URL set will guide later crawl or extract work:

   ```bash
   tvly-hikari map https://example.com/docs --json -o tavily-map.json
   ```

4. Feed only relevant discovered URLs into later extract or crawl calls.

## Hikari Notes

- Requests go to the configured Hikari `/api/tavily/map` facade.
- Use a Hikari token, not an official Tavily API key.
- Hikari centralizes quota, audit, and upstream key selection.
