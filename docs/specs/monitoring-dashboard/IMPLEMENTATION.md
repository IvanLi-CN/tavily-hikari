# Monitoring dashboard implementation plan

## 状态

- Status: repository implementation complete; 101 deployment remains owner-authorized work
- Implementation: M1-M2 complete, M3 has a deployment checklist and generic overlay/profile
- Scope: M1-M3 metrics integration and 101 instance adaptation; M4 tracing/hotpath remains optional

## Repository surfaces

- `src/observability.rs`：recorder、label allowlist、exposition、configuration and bounded flush boundary；source events use a fixed-capacity non-blocking queue, and only exporter-side flushing touches Prometheus vectors。
- `src/server/serve.rs`：conditionally mount `/metrics` without changing public metrics or business routes。
- `src/store/sqlite_runtime.rs` and `src/tavily_proxy/proxy_forward_proxy_maintenance.rs`：record SQLite events plus relay-candidate and direct-fallback upstream events without copying request content。
- `ops/observability/prometheus.yml`：generic scrape job template with token file and deployment substitutions。
- `ops/observability/recording-rules.yml`：stable `hikari:` recording rule names and classic Histogram queries。
- `ops/observability/grafana/provisioning/`：read-only datasource, dashboard provider and six Grafana managed alert rules。
- `ops/observability/grafana/dashboards/`：four dashboard JSON files with stable UIDs and deployment variables。
- `ops/observability/compose.yml` or an equivalent optional profile：Prometheus/Grafana integration for local or self-hosted environments, with immutable default image digests and deployment-owned environment overrides, without requiring it for the application alone。
- `scripts/test-observability-config.sh`：static provisioning and dashboard schema checks, with a PyYAML-backed alert identity/query validator, pinned `promtool` validation required in CI and an explicit local fallback when the tool is unavailable。

## Configuration contract

Implementation must expose the names and semantics in `SPEC.md`: `OBSERVABILITY_ENABLED`, `METRICS_BIND`, `METRICS_ALLOW_UNSPECIFIED_BIND`, `METRICS_TOKEN_FILE`, `OBSERVABILITY_ENVIRONMENT`, `OBSERVABILITY_INSTANCE` and optional `OBSERVABILITY_ROUTE_ALLOWLIST`.

Defaults must keep observability disabled and must not contain 101 domains, internal addresses, secret paths, host ports, retention values or notification destinations.

## 101 adaptation checklist

The deployed instance should be adapted outside the application repository by the owner-approved deployment change:

- Add Hikari to the existing `obs` private network while retaining its business and reverse-proxy networks.
- Mount a read-only application metrics token into Hikari and a separately managed scrape credential into Prometheus when the deployment requires different identities.
- Add one Prometheus job targeting Hikari's internal exporter address, with an explicit service/environment/instance label set and sample/cardinality limits.
- Provision the four Hikari dashboards and recording rules into the existing Grafana organization using stable datasource and dashboard UIDs.
- Keep Prometheus, exporter and Tempo ingestion/query endpoints private; do not add a public Traefik route for `/metrics`.
- Validate target freshness, sample volume, query semantics, dashboard rendering and business isolation before declaring the instance adapted.

The concrete 101 domain, network, secret file and port values remain deployment-card facts and must not be copied into this repository.

## Verification sequence

1. Unit-test label allowlists, enum rejection, recorder bounds, Histogram units and exposition output.
2. Test `/metrics` with observability disabled, valid token, missing token, invalid token and unreadable token.
3. Generate mock requests, retries, SQLite waits, task outcomes and memory samples; verify counters, buckets, reset behavior and dropped-sample metrics.
4. Run Prometheus config/rule validation and query fixtures for `up`, freshness, sample count, Counter reset and classic Histogram quantiles.
5. Provision Grafana in an isolated environment and verify all four dashboard UIDs, datasource ownership, empty/low-sample/stale states and alert rule loading.
6. Stop Prometheus, Grafana and the exporter independently; verify proxy requests, ACK, tasks and SQLite writes remain successful.
7. Run the 101 deployment acceptance only after a deployment card and owner-approved remote change exist; preserve logs and rollback evidence outside this repository.

## Repository verification

- `cargo fmt --check`, `cargo check`, `cargo clippy --locked -j 2 -- -D warnings` and `cargo build` pass on the candidate.
- The observability unit tests cover disabled no-op behavior, Bearer authentication, fixed dimensions and bounded unknown values.
- `scripts/test-observability-config.sh` validates the generic Prometheus/Grafana files, immutable default image digests, all four dashboard UIDs and the six managed alert query contracts; CI installs pinned PyYAML and Prometheus releases and fails closed if `promtool` is missing.
- Local HTTP smoke validation covers valid and invalid Bearer tokens, business health during exporter bind failure, Prometheus text output and sensitive-value absence against a mock upstream.
- The 101 target, Grafana rendering and remote outage-isolation checks remain deployment acceptance work and are intentionally not run against production from this repository.

## Open implementation risks

- Existing runtime logs already cover many performance phases; instrumentation must reuse event boundaries without double-counting the same source event.
- SQLite and request-log hot paths are sensitive to additional synchronous work; recorder updates use fixed label normalization and a lock-free bounded admission table, and must never perform database reads or extend the SQLite workload critical section.
- The public metrics API and operational metrics have different privacy and cache contracts; route registration and tests must keep them separate.
- Current production is `HA_MODE=single`; HA labels and channel metrics must represent unavailable/none explicitly rather than fabricate standby samples.
