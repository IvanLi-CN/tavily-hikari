---
name: exa-hikari-answer
description: Get cited answers through Tavily Hikari with the Exa facade.
---

# Exa Hikari Answer

Use this skill when the task needs a synthesized answer with citations from live web data.

## Workflow

1. Write the question as a standalone sentence.
2. Run:

   ```bash
   ORIGIN="https://<your-hikari-host>"
   curl -s -X POST "$ORIGIN/api/exa/answer" \
     -H "x-api-key: th-<id>-<secret>" \
     -H "Content-Type: application/json" \
     -d '{"query": "How do MCP sessions work?", "model": "exa"}'
   ```

3. Pick the model: `exa`, `exa-pro`, `exa-research`, or `exa-fast`.
4. Treat the answer as a draft; verify each claim against the citations before relaying it.

## Boundaries

- Never streams. `stream: true` (or any truthy value) is rejected with a typed 400 before any
  upstream call. Omit `stream` or set it to `false`.
- The only Exa answer route is `/api/exa/answer`.

## Hikari Notes

- Base URL: `https://<your-hikari-host>/api/exa` mirrors `https://api.exa.ai`.
- Auth slots: `x-api-key: th-...` or `Authorization: Bearer th-...`.
- Hikari bills 2 credits per successful answer; failures and stream rejections bill 0.
- MCP clients can use `https://<your-hikari-host>/exa/mcp`; tool `exa_answer` (a Hikari-local
  name, since Exa retired its hosted answering tool) bills 2 credits and never sends `stream`.
