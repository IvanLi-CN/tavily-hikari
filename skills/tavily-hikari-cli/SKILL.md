---
name: tavily-hikari-cli
description: Run Tavily CLI workflows through a configured Tavily Hikari deployment.
---

# Tavily Hikari CLI

Use this skill when a task needs Tavily search, extract, crawl, map, or research through a Tavily
Hikari deployment.

## Workflow

1. Verify the wrapper is configured:

   ```bash
   tvly-hikari doctor
   ```

2. Run official Tavily CLI commands through the wrapper:

   ```bash
   tvly-hikari search "agentic browser automation patterns" --json
   tvly-hikari extract https://example.com --json
   tvly-hikari crawl https://example.com --json
   tvly-hikari map https://example.com --json
   tvly-hikari research "compare MCP and CLI agent tools" --json
   ```

3. Prefer `--json` for machine-readable output and pass through official `tvly` flags such as
   `-o` or `--output-dir` when the task needs saved artifacts.

## Hikari Contract

- `tvly-hikari` injects `TAVILY_API_BASE_URL=<baseUrl>/api/tavily`.
- `tvly-hikari` injects `TAVILY_API_KEY=<Hikari token>`.
- The Hikari token is not an official Tavily API key.
- Hikari performs quota checks, audit logging, and upstream key-pool routing.
