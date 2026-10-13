# Hikari observability deployment

This directory contains the generic Prometheus/Grafana provisioning contract for a Hikari instance. Observability is optional and disabled by default in the application.

## Application settings

Set `OBSERVABILITY_ENABLED=true`, choose an explicit `METRICS_BIND` address, and mount a read-only `METRICS_TOKEN_FILE`. `OBSERVABILITY_ENVIRONMENT` and `OBSERVABILITY_INSTANCE` identify the deployment in the surrounding target configuration. `OBSERVABILITY_ROUTE_ALLOWLIST` may restrict request route families; it never accepts arbitrary label values.

The exporter is a separate listener and is not a public application route. It accepts only `Authorization: Bearer <token>` and returns 401 for a missing or mismatched token. A missing token file, invalid bind, or failed exporter startup leaves the business listener running.

## Prometheus target

Copy `file_sd/hikari/example.yml` to an instance-owned target file and replace the example address and labels. The scrape job uses `honor_labels: true`, so the exporter’s validated `service`, `environment`, and `instance` const labels remain canonical; keep target labels aligned for target discovery and inventory. Keep the target file and `secrets/hikari-metrics.token` outside this repository when they contain deployment-specific values. The scrape job uses the bearer token file and refreshes file-SD targets without requiring a Prometheus restart.

## Grafana provisioning

The datasource UID is `hikari-prometheus`; dashboard UIDs are `hikari-overview`, `hikari-proxy`, `hikari-storage`, and `hikari-runtime`. Six Grafana managed alert rules are provisioned from `grafana/provisioning/alerting/alert-rules.yml`; notification policies and contact points remain deployment-owned. Mount the provisioning directory and dashboard directory read-only. UI edits are not the configuration source of truth.

## Token rotation and failure handling

1. Write the new token to a new owner-controlled secret file with restrictive permissions.
2. Update the application and Prometheus mounts together, then restart or reload both processes according to the deployment manager.
3. Verify a valid scrape returns 200, an old token returns 401, the target is UP, and business `/health` remains 200.
4. If exporter, Prometheus, or Grafana is unavailable, investigate the unavailable/stale alert; business traffic and SQLite persistence must continue independently.

The generic files intentionally do not contain a domain, internal address, host port, retention policy, notification destination, or real credential.
