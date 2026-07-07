# Tavily Hikari Search

Use this skill when the task needs web search results through Tavily Hikari.

## Workflow

1. Shape a focused query with any needed freshness, domain, or result-count constraints.
2. Run:

   ```bash
   tvly-hikari search "latest MCP client setup patterns" --json
   ```

3. For saved output, pass official CLI output flags through the wrapper:

   ```bash
   tvly-hikari search "Tavily CLI environment variables" --json -o tavily-search.json
   ```

4. Summarize from the JSON result and cite URLs when the answer depends on external facts.

## Hikari Notes

- Requests go to the configured Hikari `/api/tavily/search` facade.
- Use the Hikari token from `tvly-hikari configure`; do not use a raw Tavily API key.
- Hikari records audit logs and enforces the token's quota before forwarding upstream.
