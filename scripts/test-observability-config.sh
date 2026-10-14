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
grep -Fq 'honor_labels: false' "$observability_dir/prometheus.yml"
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
  promtool check config "$observability_dir/prometheus.yml"
  promtool check rules "$observability_dir/recording-rules.yml"
else
  echo "promtool not installed; checked files and dashboard JSON only"
fi

echo "observability configuration checks passed"
