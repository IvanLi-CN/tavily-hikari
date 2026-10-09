---
name: brave-hikari-llm-context
description: Build token-bounded LLM context through Tavily Hikari with the Brave facade.
---

# Brave Hikari LLM Context

Use this skill when the task needs pre-assembled, token-bounded context blocks for a query.

## Workflow

1. Set the context budget with `maximum_number_of_tokens` and cap the source URLs with
   `maximum_number_of_urls`.
2. Run:

   ```bash
   ORIGIN="https://<your-hikari-host>"
   curl -s "$ORIGIN/api/brave/res/v1/llm/context?q=mcp+session+affinity&count=10" \
     -H "X-Subscription-Token: th-<id>-<secret>"
   ```

   Add `&maximum_number_of_tokens=4096` to pin the token budget.
3. Feed the returned context into the model prompt and keep the source list for citation.

## Boundaries

- Context assembly only. No URL extraction, crawl, map, or summarizer endpoints.
- Bounds: `count` 1-50, `maximum_number_of_urls` 1-50, `maximum_number_of_tokens` 1024-32768.
- `context_threshold_mode` accepts `low`, `medium`, or `high`.

## Hikari Notes

- Base URL: `https://<your-hikari-host>/api/brave` mirrors `https://api.search.brave.com`.
- Auth slots: `X-Subscription-Token: th-...` or `Authorization: Bearer th-...`.
- Hikari bills 2 credits per successful context call (the highest Brave tier).
- MCP clients can use `https://<your-hikari-host>/brave/mcp`; tool `brave_llm_context` bills 2
  credits.
