---
status: accepted
---

# Separate runtime metrics from business persistence

Tavily Hikari will expose opt-in, token-protected runtime metrics from an in-process bounded recorder, while Prometheus owns metric history and Grafana owns operational queries, dashboards and rules. The existing SQLite database remains the source of business facts, and the public aggregate metrics API remains a separate anonymous JSON interface. This boundary avoids adding synchronous observability writes to SQLite, prevents dashboard availability from becoming a proxy success condition, and lets the same application contract run with or without the shared 101 observability stack.

## Considered options

- Reuse the public JSON metrics endpoint as a Prometheus scrape surface: rejected because its privacy, cache, CORS and aggregate field contract is intentionally public-facing and does not expose runtime diagnostics.
- Persist Prometheus-style samples in the business SQLite database: rejected because it increases write/read pressure and couples observability retention to business recovery and backup semantics.
- Make Grafana UI the configuration source: rejected because dashboards and alert rules would drift from the repository and become difficult to reproduce.
