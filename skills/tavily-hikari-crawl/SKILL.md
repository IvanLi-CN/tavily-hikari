# Tavily Hikari Crawl

Use this skill when a task needs to crawl a site or path through Tavily Hikari.

## Workflow

1. Define the start URL and crawl bounds before running the command.
2. Run a bounded crawl:

   ```bash
   tvly-hikari crawl https://example.com/docs --json
   ```

3. Save crawl results for follow-up analysis:

   ```bash
   tvly-hikari crawl https://example.com/docs --json --output-dir tavily-crawl
   ```

4. Prefer narrow crawl scope over broad site-wide crawling.

## Hikari Notes

- Requests go to the configured Hikari `/api/tavily/crawl` facade.
- Hikari token quota and audit logs apply to the crawl.
- The upstream Tavily key remains inside Hikari's key pool.
