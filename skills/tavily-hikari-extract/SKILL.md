# Tavily Hikari Extract

Use this skill when the task needs content extraction from one or more URLs through Tavily Hikari.

## Workflow

1. Confirm the URL list is specific and avoids private or irrelevant pages.
2. Run:

   ```bash
   tvly-hikari extract https://example.com/article --json
   ```

3. Save larger extraction output when it will be reused:

   ```bash
   tvly-hikari extract https://example.com/article --json -o tavily-extract.json
   ```

4. Use extracted text as supporting evidence, keeping quotes brief and source-specific.

## Hikari Notes

- Requests go to the configured Hikari `/api/tavily/extract` facade.
- The configured token is a Hikari access token.
- Hikari applies quota, audit logging, and upstream key-pool routing.
