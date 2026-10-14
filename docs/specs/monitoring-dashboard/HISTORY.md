# Monitoring dashboard history

## 2026-10-14

- Created the requirements contract after a three-round design interview using the Monitoring dashboard playbook, the local repository facts and a read-only inspection of the deployed 101 observability stack.
- Chose opt-in, token-protected application metrics with Prometheus/Grafana provisioning in the repository and a separate 101 instance adaptation.
- Fixed the first metric namespace as `hikari_`, classic Histogram semantics, a low-cardinality label allowlist and four operational dashboard entry points.
- Kept tracing and hotpath as independent follow-up capabilities and preserved the existing public aggregate metrics API as a separate interface.
- Clarified that the existing runtime-performance-observability spec owns structured logs, low-memory behavior and SQLite diagnostics; this spec owns exporter and dashboard integration.
