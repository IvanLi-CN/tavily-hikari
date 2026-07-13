---
name: tavily-hikari-best-practices
description: Guide agents to use Tavily Hikari safely through tvly-hikari.
---

# Tavily Hikari Best Practices

Use this skill when deciding how to call Tavily Hikari from an agent workflow.

## Rules

- Use `tvly-hikari ... --json` for agent-readable output.
- Use Hikari tokens beginning with `th-`; never paste official Tavily API keys into downstream
  agents or user machines.
- Configure Hikari once:

  ```bash
  tvly-hikari configure --base-url https://<your-hikari-host> --token th-<id>-<secret>
  ```

- Treat `/mcp` and `/api/tavily/*` as different integration surfaces:
  - MCP clients use `https://<your-hikari-host>/mcp` with bearer auth.
  - CLI and HTTP-style clients use `https://<your-hikari-host>/api/tavily`.
- Keep searches narrow, save large outputs with `-o` or `--output-dir`, and cite source URLs when
  answering factual questions.
- Do not test against Tavily production upstream unless the operator explicitly approves it; use
  local or sandbox Hikari endpoints for smoke tests.

## Quick Smoke

```bash
tvly-hikari doctor
tvly-hikari search "Tavily Hikari smoke test" --json
```
