#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
observability_dir="$repo_root/ops/observability"

required_files=(
  "$observability_dir/prometheus.yml"
  "$observability_dir/recording-rules.yml"
  "$observability_dir/grafana/provisioning/datasources/datasource.yml"
  "$observability_dir/grafana/provisioning/dashboards/provider.yml"
  "$observability_dir/grafana/provisioning/alerting/alert-rules.yml"
  "$observability_dir/grafana/dashboards/hikari-overview.json"
  "$observability_dir/grafana/dashboards/hikari-proxy.json"
  "$observability_dir/grafana/dashboards/hikari-storage.json"
  "$observability_dir/grafana/dashboards/hikari-runtime.json"
)

for file in "${required_files[@]}"; do
  test -s "$file"
done

alert_rules="$observability_dir/grafana/provisioning/alerting/alert-rules.yml"
grep -Fq 'apiVersion: 1' "$alert_rules"
grep -Fq 'datasourceUid: hikari-prometheus' "$alert_rules"
test "$(grep -c '^      - uid: hikari-' "$alert_rules")" -eq 6
test "$(grep -c 'up{job="tavily-hikari"} == bool 1' "$alert_rules")" -eq 5
test "$(grep -c 'hikari:sample_count5m >= bool' "$alert_rules")" -eq 4
grep -Fq 'unless on (service, environment, instance) hikari:sample_count5m >= 1' "$alert_rules"
grep -Fq 'honor_labels: false' "$observability_dir/prometheus.yml"
grep -Fq 'HIKARI_OBSERVABILITY_FILE_SD_DIR:?Set HIKARI_OBSERVABILITY_FILE_SD_DIR' "$observability_dir/compose.yml"
grep -Fq 'HIKARI_OBSERVABILITY_SECRETS_DIR:?Set HIKARI_OBSERVABILITY_SECRETS_DIR' "$observability_dir/compose.yml"
test ! -e "$observability_dir/file_sd/hikari/example.yml"
test -s "$observability_dir/file_sd/hikari/example.yml.disabled"
grep -Fq 'body_size_limit: 2MB' "$observability_dir/prometheus.yml"
grep -Fq 'sample_limit: 2000' "$observability_dir/prometheus.yml"
grep -Fq 'label_limit: 32' "$observability_dir/prometheus.yml"
grep -Fq 'label_name_length_limit: 64' "$observability_dir/prometheus.yml"
grep -Fq 'label_value_length_limit: 128' "$observability_dir/prometheus.yml"

python3 - "$observability_dir/grafana/dashboards" <<'PY'
import json
import pathlib
import sys

dashboard_dir = pathlib.Path(sys.argv[1])
expected = {
    "hikari-overview.json": "hikari-overview",
    "hikari-proxy.json": "hikari-proxy",
    "hikari-storage.json": "hikari-storage",
    "hikari-runtime.json": "hikari-runtime",
}
for filename, uid in expected.items():
    payload = json.loads((dashboard_dir / filename).read_text())
    assert payload["uid"] == uid
    assert payload["title"].startswith("Hikari ")
PY

if command -v promtool >/dev/null 2>&1; then
  validation_root="$(mktemp -d)"
  trap 'rm -rf "$validation_root"' EXIT
  python3 - "$observability_dir/prometheus.yml" "$observability_dir/recording-rules.yml" "$validation_root" <<'PY'
import pathlib
import sys

prometheus_source = pathlib.Path(sys.argv[1])
rules_source = pathlib.Path(sys.argv[2])
validation_root = pathlib.Path(sys.argv[3])
(validation_root / "rules").mkdir(parents=True)
(validation_root / "secrets").mkdir()
(validation_root / "file_sd" / "hikari").mkdir(parents=True)
(validation_root / "rules" / "hikari-recording-rules.yml").write_text(
    rules_source.read_text()
)
(validation_root / "secrets" / "hikari-metrics.token").write_text(
    "local-validation-token\n"
)
(validation_root / "file_sd" / "hikari" / "validation.yml").write_text(
    "- targets:\n    - 127.0.0.1:1\n"
)
config = prometheus_source.read_text()
config = config.replace(
    "/etc/prometheus/rules/hikari-recording-rules.yml",
    str(validation_root / "rules" / "hikari-recording-rules.yml"),
)
config = config.replace(
    "/etc/prometheus/secrets/hikari-metrics.token",
    str(validation_root / "secrets" / "hikari-metrics.token"),
)
config = config.replace(
    "/etc/prometheus/file_sd/hikari/*.yml",
    str(validation_root / "file_sd" / "hikari" / "*.yml"),
)
(validation_root / "prometheus.yml").write_text(config)
PY
  promtool check config "$validation_root/prometheus.yml"
  promtool check rules "$observability_dir/recording-rules.yml"
else
  if [[ "${OBSERVABILITY_REQUIRE_PROMTOOL:-false}" == "true" ]]; then
    echo "promtool is required for this validation environment" >&2
    exit 1
  fi
  echo "promtool not installed; checked files and dashboard JSON only"
fi

echo "observability configuration checks passed"
