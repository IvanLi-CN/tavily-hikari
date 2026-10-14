# Hikari observability deployment

This directory contains the generic Prometheus/Grafana provisioning contract for a Hikari instance. Observability is optional and disabled by default in the application.

## Application settings

Set `OBSERVABILITY_ENABLED=true`, choose an explicit private `METRICS_BIND` address, and mount a read-only `METRICS_TOKEN_FILE`. `METRICS_ALLOW_UNSPECIFIED_BIND` remains false unless the deployment network is explicitly private and reviewed. `OBSERVABILITY_ENVIRONMENT` accepts only the documented environment enum, and `OBSERVABILITY_INSTANCE` accepts only the documented deployment slots or numeric Hikari ordinals. `OBSERVABILITY_ROUTE_ALLOWLIST` may restrict request route families; it never accepts arbitrary label values.

The exporter is a separate listener and is not a public application route. It accepts only `Authorization: Bearer <token>` and returns 401 for a missing or mismatched token. A missing token file, invalid bind, or failed exporter startup leaves the business listener running.

## Prometheus target

Copy `file_sd/hikari/example.yml.disabled` to an instance-owned `.yml` target file and replace the example address and labels. Set `HIKARI_OBSERVABILITY_FILE_SD_DIR` and `HIKARI_OBSERVABILITY_SECRETS_DIR` to deployment-owned directories before starting `compose.yml`; the compose file refuses to start without those explicit paths, so target files and bearer tokens are not accidentally created under the repository. The scrape job keeps `honor_labels: false`, so Prometheus target labels remain canonical inventory labels and conflicting scraped labels are renamed rather than allowed to spoof target identity; keep target `service`, `environment`, and `instance` aligned with the exporter labels. The scrape job uses the bearer token file and refreshes file-SD targets without requiring a Prometheus restart.

## Grafana provisioning

The datasource UID is `hikari-prometheus`; dashboard UIDs are `hikari-overview`, `hikari-proxy`, `hikari-storage`, and `hikari-runtime`. Six Grafana managed alert rules are provisioned from `grafana/provisioning/alerting/alert-rules.yml`; notification policies and contact points remain deployment-owned. Mount the provisioning directory and dashboard directory read-only. UI edits are not the configuration source of truth.

## Diagnostic workflow

Start with **Hikari Overview** and select the service, environment, and instance under investigation. Target health and `Samples / 5m` establish whether the exporter is reachable and whether the percentile values have enough traffic behind them; a down target or zero samples should be resolved before interpreting application trends.

For a request problem, compare **Request rate by route family**, **Error rate by route family**, **Request latency percentiles**, and **Request phase P95**. This separates a traffic mix change from a tail-latency regression and points to admission, upstream, persistence, or acknowledgement work without adding request-specific labels.

Use **Hikari Proxy** when the phase view points at upstream work or when request latency rises without local resource pressure. **Attempts / request**, upstream error rate, operation-level P95, and the P50/P95/P99 spread distinguish retries and dependency failures from a uniformly slow dependency.

Use **Hikari Storage** when persistence or background work is implicated. The SQLite wait percentiles and wait-reason panel show lock or coordinator contention; transaction and task outcome panels show whether the contention is affecting foreground requests or only asynchronous work.

Use **Hikari Runtime** to separate application pressure from observability pressure. Memory utilization and components show RSS against the observed limit, while CPU, dropped samples, and exporter flush outcomes show whether the runtime or bounded recorder is losing signal. Non-zero dropped samples mean the other panels are incomplete evidence.

An intentional `No data` panel means the selected label set has no samples for that event class in the time range; it is different from a down target. The dashboards retain fixed low-cardinality labels and never require user, token, IP, request ID, URL, query, body, SQL, or error text.

## Token rotation and failure handling

1. Write the new token to a new owner-controlled secret file with restrictive permissions.
2. Update the application and Prometheus mounts together, then restart or reload both processes according to the deployment manager.
3. Verify a valid scrape returns 200, an old token returns 401, the target is UP, and business `/health` remains 200.
4. If exporter, Prometheus, or Grafana is unavailable, investigate the unavailable/stale alert; business traffic and SQLite persistence must continue independently.

The generic files intentionally do not contain a domain, internal address, host port, retention policy, notification destination, or real credential.
