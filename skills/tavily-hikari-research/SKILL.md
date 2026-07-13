---
name: tavily-hikari-research
description: Run multi-step Tavily research through Tavily Hikari with tvly-hikari.
---

# Tavily Hikari Research

Use this skill when the task needs multi-step Tavily research through Tavily Hikari.

## Workflow

1. Write a concise research question with scope, date, geography, and source constraints.
2. Run:

   ```bash
   tvly-hikari research "Compare Tavily CLI, Agent Skills, and MCP for AI agents" --json
   ```

3. Save the response for longer investigations:

   ```bash
   tvly-hikari research "Compare Tavily CLI, Agent Skills, and MCP for AI agents" --json -o tavily-research.json
   ```

4. If the official CLI returns an async request id, inspect or wait for the result through the
   official follow-up commands exposed by the wrapper:

   ```bash
   tvly-hikari research status <request-id> --json
   tvly-hikari research poll <request-id> --json -o tavily-research.json
   ```

## Hikari Notes

- Requests go to the configured Hikari `/api/tavily/research` facade.
- Hikari token quota applies to research calls.
- Hikari audit logs retain the downstream token context while upstream keys remain private.
