#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
observability_dir="$repo_root/ops/observability"

required_files=(
  "$observability_dir/prometheus.yml"
  "$observability_dir/recording-rules.yml"
  "$observability_dir/compose.yml"
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
test "$(grep -c 'up{job="tavily-hikari"} == 1' "$alert_rules")" -eq 5
test "$(grep -c 'hikari:sample_count5m >= 10' "$alert_rules")" -eq 3
test "$(grep -Ec 'hikari:sample_count5m >= 1([^0-9]|$)' "$alert_rules")" -eq 2
grep -Fq 'unless on (service, environment, instance) hikari:sample_count5m >= 1' "$alert_rules"
if grep -Eq 'and on \(service, environment, instance\).*\bbool\b' "$alert_rules"; then
  echo "alert set filters must not use bool comparisons" >&2
  exit 1
fi
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
grep -Fq 'prom/prometheus@sha256:2659f4c2ebb718e7695cb9b25ffa7d6be64db013daba13e05c875451cf51b0d3' "$observability_dir/compose.yml"
grep -Fq 'grafana/grafana@sha256:408afb9726de5122b00a2576763a8a57a3c86d5b0eff5305bc994ceb3eb96c3f' "$observability_dir/compose.yml"
python3 "$repo_root/scripts/validate-observability-alerts.py" "$alert_rules"

python3 - "$observability_dir/compose.yml" <<'PY'
import sys
from pathlib import Path

import yaml

compose = yaml.safe_load(Path(sys.argv[1]).read_text(encoding="utf-8"))
services = compose.get("services")
if not isinstance(services, dict):
    raise SystemExit("compose.yml must define services")
expected = {
    "prometheus": "prom/prometheus@sha256:2659f4c2ebb718e7695cb9b25ffa7d6be64db013daba13e05c875451cf51b0d3",
    "grafana": "grafana/grafana@sha256:408afb9726de5122b00a2576763a8a57a3c86d5b0eff5305bc994ceb3eb96c3f",
}
for name, default_image in expected.items():
    image = services.get(name, {}).get("image")
    if not isinstance(image, str) or default_image not in image:
        raise SystemExit(f"{name}: immutable default image digest is missing")
PY

python3 - "$alert_rules" <<'PY'
import re
import sys
from pathlib import Path

alert_text = Path(sys.argv[1]).read_text(encoding="utf-8")
blocks = [block for block in alert_text.split("\n      - uid: ")[1:] if block.strip()]
if len(blocks) != 6:
    raise SystemExit(f"expected six alert rule blocks, found {len(blocks)}")
expected_uids = {
    "hikari-exporter-unavailable",
    "hikari-scrape-stale",
    "hikari-error-rate-high",
    "hikari-request-p95-high",
    "hikari-sqlite-wait-high",
    "hikari-memory-headroom-low",
}
required_query_fragments = {
    "hikari-exporter-unavailable": ('up{job="tavily-hikari"} == bool 0',),
    "hikari-scrape-stale": (
        'up{job="tavily-hikari"} == 1',
        "hikari:sample_count5m >= 1",
    ),
    "hikari-error-rate-high": (
        'up{job="tavily-hikari"} == 1',
        "hikari:sample_count5m >= 10",
        "hikari:error_rate5m > 0.2",
        "hikari:request_rate5m > 0.1",
    ),
    "hikari-request-p95-high": (
        'up{job="tavily-hikari"} == 1',
        "hikari:sample_count5m >= 10",
        "hikari:request_p95_seconds5m > 2",
    ),
    "hikari-sqlite-wait-high": (
        'up{job="tavily-hikari"} == 1',
        "hikari:sample_count5m >= 10",
        "hikari:sqlite_wait_p95_seconds5m > 0.5",
    ),
    "hikari-memory-headroom-low": (
        'up{job="tavily-hikari"} == 1',
        "hikari:sample_count5m >= 1",
        "hikari:memory_headroom_bytes < 134217728",
    ),
}
seen_uids = []

for block in blocks:
    uid, _, body = block.partition("\n")
    if uid not in expected_uids or uid in seen_uids:
        raise SystemExit(f"unexpected or duplicate alert uid: {uid}")
    seen_uids.append(uid)
    for pattern, expected, description in (
        (r"^        condition: C$", 1, "condition C"),
        (r"^          - refId: A$", 1, "source refId A"),
        (r"^                type: prometheus$", 1, "source model type"),
        (r"^                uid: hikari-prometheus$", 1, "source model datasource UID"),
        (r"^              refId: A$", 1, "source model refId A"),
        (r"^              expr: .+$", 1, "source PromQL expression"),
        (r"^          - refId: C$", 1, "threshold refId C"),
        (r"^                type: __expr__$", 1, "threshold model type"),
        (r"^                uid: __expr__$", 1, "threshold model datasource UID"),
        (r"^              expression: A$", 1, "threshold expression A"),
        (r"^                    params: \[A\]$", 1, "threshold query A"),
        (r"^              type: threshold$", 1, "threshold node"),
        (r"^            datasourceUid: hikari-prometheus$", 1, "Prometheus datasource"),
        (r"^            datasourceUid: __expr__$", 1, "expression datasource"),
    ):
        actual = len(re.findall(pattern, body, flags=re.MULTILINE))
        if actual != expected:
            raise SystemExit(f"{uid}: expected one {description}, found {actual}")
    if re.search(r"^                    params: \[C\]$", body, flags=re.MULTILINE):
        raise SystemExit(f"{uid}: threshold query is self-referential")
    lines = body.splitlines()
    expr_lines = [index for index, line in enumerate(lines) if line.startswith("              expr:")]
    if len(expr_lines) != 1:
        raise SystemExit(f"{uid}: expected one source PromQL expr field")
    expr_line = lines[expr_lines[0]].split("expr:", 1)[1].strip()
    if expr_line in {">-", "|-", ">", "|"}:
        folded = []
        for continuation in lines[expr_lines[0] + 1 :]:
            if continuation.strip() and len(continuation) - len(continuation.lstrip(" ")) <= 14:
                break
            if continuation.strip() and not continuation.lstrip().startswith("#"):
                folded.append(continuation.strip())
        expr_line = " ".join(folded)
    if not expr_line:
        raise SystemExit(f"{uid}: source PromQL expr is empty")
    for fragment in required_query_fragments[uid]:
        if fragment not in expr_line:
            raise SystemExit(f"{uid}: source PromQL is missing {fragment!r}")
if set(seen_uids) != expected_uids:
    raise SystemExit("alert rule UID set does not match the six managed Hikari alerts")
PY

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
  python3 "$repo_root/scripts/validate-observability-alerts.py" \
    "$alert_rules" \
    --prometheus-rules-output "$validation_root/grafana-alert-expressions.yml"
  promtool check rules "$validation_root/grafana-alert-expressions.yml"
else
  if [[ "${OBSERVABILITY_REQUIRE_PROMTOOL:-false}" == "true" ]]; then
    echo "promtool is required for this validation environment" >&2
    exit 1
  fi
  echo "promtool not installed; checked files, alert provisioning, and dashboard JSON only"
fi

echo "observability configuration checks passed"
