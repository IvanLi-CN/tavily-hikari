#!/usr/bin/env bash
set -euo pipefail

show_help() {
  cat <<'EOF'
Usage: run_snapshot_comparison.sh

Run isolated baseline and candidate performance checks against a copied 101 core/observability
SQLite snapshot. The caller must provide repositories and a snapshot under one owned REMOTE_RUN.

Required environment:
  REMOTE_RUN        Isolated /srv/codex run directory
  CANDIDATE_REPO    Candidate source tree within REMOTE_RUN
  BASELINE_REPO     Baseline source tree within REMOTE_RUN
  SNAPSHOT_DIR      Directory containing manifest.env and compressed core/observability snapshots
  COMPOSE_PROJECT   Unique Docker Compose project name

Optional environment:
  DURATION_SECS     Per-variant duration, defaults to 600
EOF
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  show_help
  exit 0
fi

REMOTE_RUN="${REMOTE_RUN:?REMOTE_RUN is required}"
CANDIDATE_REPO="${CANDIDATE_REPO:?CANDIDATE_REPO is required}"
BASELINE_REPO="${BASELINE_REPO:?BASELINE_REPO is required}"
SNAPSHOT_DIR="${SNAPSHOT_DIR:?SNAPSHOT_DIR is required}"
COMPOSE_PROJECT="${COMPOSE_PROJECT:?COMPOSE_PROJECT is required}"
DURATION_SECS="${DURATION_SECS:-600}"
ARTIFACTS_DIR="${REMOTE_RUN}/artifacts/performance-recovery"
WORK_DIR="${REMOTE_RUN}/performance-recovery"
CANDIDATE_SHA="${CANDIDATE_SHA:-unknown}"
BASELINE_SHA="${BASELINE_SHA:-unknown}"
# Testbox retries must not ask Docker Hub to resolve a mutable tag. This digest
# is the exact Rust 1.91 Bookworm image used by the checked-in test Dockerfile.
TESTBOX_RUST_BASE_IMAGE="rust:1.91-bookworm@sha256:c1e5f19e773b7878c3f7a805dd00a495e747acbdc76fb2337a4ebf0418896b33"

# These lists deliberately mirror the HA event-table allowlists. The recovery
# gate must only require progress on data the online GC may legally delete;
# raw legacy rows outside the replication contract are handled only by the
# bounded invalid-resource cursor and must never be treated as retention debt.
HA_GC_CONTROL_RESOURCES="'admin_password_settings', 'announcements', 'account_entitlements', 'api_key_low_quota_depletions', 'api_key_maintenance_records', 'api_key_quarantines', 'api_keys', 'auth_tokens', 'forward_proxy_settings', 'linuxdo_credit_recharge_entitlements', 'linuxdo_credit_recharge_orders', 'meta', 'oauth_accounts', 'upstream_reconciliation_control_state', 'upstream_reconciliation_control_transitions', 'token_api_key_bindings', 'user_api_key_bindings', 'user_tag_bindings', 'user_tags', 'user_token_bindings', 'users'"
HA_GC_BILLING_RESOURCES="'billing_ledger', 'billing_reconciliation_adjustments'"
HA_GC_RUNTIME_RESOURCES="'account_monthly_quota', 'account_quota_limits', 'account_usage_buckets', 'auth_token_quota', 'forward_proxy_key_affinity', 'forward_proxy_node_overrides', 'http_project_api_key_affinity', 'mcp_sessions', 'research_requests', 'token_primary_api_key_affinity', 'token_usage_buckets', 'upstream_reconciliation_research', 'upstream_reconciliation_settlements', 'upstream_reconciliation_usage', 'upstream_reconciliation_work', 'upstream_usage_rate_attempts', 'user_primary_api_key_affinity'"

manifest_get() {
  local key="$1"
  awk -F= -v target="$key" '$1 == target { sub($1"=", ""); print; exit }' \
    "$SNAPSHOT_DIR/manifest.env"
}

MANIFEST_PATH="$SNAPSHOT_DIR/manifest.env"
[[ -f "$MANIFEST_PATH" ]] || { echo "missing snapshot manifest" >&2; exit 2; }
CORE_COMPRESSED_NAME="$(manifest_get core_compressed_snapshot_name)"
SIDECAR_COMPRESSED_NAME="$(manifest_get sidecar_compressed_snapshot_name)"
CORE_SNAPSHOT_SHA256="$(manifest_get core_snapshot_sha256)"
SIDECAR_SNAPSHOT_SHA256="$(manifest_get sidecar_snapshot_sha256)"
CORE_SNAPSHOT_PAGE_COUNT="$(manifest_get core_snapshot_page_count)"
SIDECAR_SNAPSHOT_PAGE_COUNT="$(manifest_get sidecar_snapshot_page_count)"
CORE_COMPRESSED_SNAPSHOT_SHA256="$(manifest_get core_compressed_snapshot_sha256)"
SIDECAR_COMPRESSED_SNAPSHOT_SHA256="$(manifest_get sidecar_compressed_snapshot_sha256)"

for snapshot_name in "$CORE_COMPRESSED_NAME" "$SIDECAR_COMPRESSED_NAME"; do
  [[ "$snapshot_name" =~ ^[A-Za-z0-9_.-]+$ ]] || {
    echo "invalid compressed snapshot filename in manifest" >&2
    exit 2
  }
done
for expected in \
  "$CORE_SNAPSHOT_SHA256" \
  "$SIDECAR_SNAPSHOT_SHA256" \
  "$CORE_COMPRESSED_SNAPSHOT_SHA256" \
  "$SIDECAR_COMPRESSED_SNAPSHOT_SHA256"; do
  [[ "$expected" =~ ^[0-9a-f]{64}$ ]] || {
    echo "invalid snapshot checksum in manifest" >&2
    exit 2
  }
done
for expected_pages in "$CORE_SNAPSHOT_PAGE_COUNT" "$SIDECAR_SNAPSHOT_PAGE_COUNT"; do
  [[ "$expected_pages" =~ ^[1-9][0-9]*$ ]] || {
    echo "invalid snapshot page count in manifest" >&2
    exit 2
  }
done
CORE_COMPRESSED_DB="$SNAPSHOT_DIR/$CORE_COMPRESSED_NAME"
SIDECAR_COMPRESSED_DB="$SNAPSHOT_DIR/$SIDECAR_COMPRESSED_NAME"

case "$REMOTE_RUN" in
  /srv/codex/workspaces/*/runs/*) ;;
  *) echo "REMOTE_RUN must be an isolated /srv/codex workspace run" >&2; exit 2 ;;
esac
[[ "$COMPOSE_PROJECT" =~ ^[a-z0-9][a-z0-9_-]{0,62}$ ]] || {
  echo "invalid COMPOSE_PROJECT" >&2
  exit 2
}
[[ "$DURATION_SECS" =~ ^[0-9]+$ ]] && (( DURATION_SECS >= 60 )) || {
  echo "DURATION_SECS must be at least 60" >&2
  exit 2
}
for path in "$CANDIDATE_REPO" "$BASELINE_REPO" "$CORE_COMPRESSED_DB" "$SIDECAR_COMPRESSED_DB"; do
  [[ -e "$path" ]] || { echo "missing required path: $path" >&2; exit 2; }
done

compose() {
  docker compose -p "$COMPOSE_PROJECT" -f "$WORK_DIR/compose.yml" "$@"
}

cleanup_compose() {
  compose down -v --remove-orphans >/dev/null 2>&1 || true
}

cleanup_app_image() {
  docker image rm -f "${COMPOSE_PROJECT}-app:latest" >/dev/null 2>&1 || true
}

remove_variant_data() {
  local variant_dir="$1"
  case "$variant_dir" in
    "$WORK_DIR"/baseline|"$WORK_DIR"/candidate) ;;
    *) echo "refusing to remove unexpected variant directory: $variant_dir" >&2; exit 2 ;;
  esac
  rm -rf -- "$variant_dir"
}

verify_variant_database() {
  local path="$1"
  local expected_sha256="$2"
  local expected_page_count="$3"
  local actual_sha256 actual_page_count
  actual_sha256="$(sha256sum "$path" | awk '{print $1}')"
  actual_page_count="$(sqlite3 "$path" 'PRAGMA page_count;' | tr -d '\r')"
  [[ "$actual_sha256" == "$expected_sha256" ]] || {
    echo "expanded snapshot checksum mismatch for $path" >&2
    exit 3
  }
  [[ "$actual_page_count" == "$expected_page_count" ]] || {
    echo "expanded snapshot page count mismatch for $path" >&2
    exit 3
  }
  [[ "$(sqlite3 "$path" 'PRAGMA integrity_check;' | tr -d '\r')" == "ok" ]] || {
    echo "expanded snapshot integrity check failed for $path" >&2
    exit 3
  }
}

expand_variant_data() {
  local variant_dir="$1"
  zstd -q -d -c "$CORE_COMPRESSED_DB" > "$variant_dir/tavily_proxy.db"
  zstd -q -d -c "$SIDECAR_COMPRESSED_DB" > "$variant_dir/tavily_proxy-observability.db"
  chmod 600 "$variant_dir/tavily_proxy.db" "$variant_dir/tavily_proxy-observability.db"
  verify_variant_database \
    "$variant_dir/tavily_proxy.db" \
    "$CORE_SNAPSHOT_SHA256" \
    "$CORE_SNAPSHOT_PAGE_COUNT"
  verify_variant_database \
    "$variant_dir/tavily_proxy-observability.db" \
    "$SIDECAR_SNAPSHOT_SHA256" \
    "$SIDECAR_SNAPSHOT_PAGE_COUNT"
}

capture_ha_gc_state() {
  local database_path="$1"
  local target_path="$2"
  sqlite3 -tabs "$database_path" "
    SELECT
      state.channel,
      state.total_deleted_rows,
      COALESCE(state.oldest_deletable_age_secs, -1),
      CASE state.channel
        WHEN 'control' THEN EXISTS(
          SELECT 1 FROM ha_outbox
          WHERE created_at < unixepoch() - 72 * 60 * 60
            AND resource IN ($HA_GC_CONTROL_RESOURCES)
        )
        WHEN 'billing' THEN EXISTS(
          SELECT 1 FROM ha_billing_outbox
          WHERE created_at < unixepoch() - 14 * 24 * 60 * 60
            AND resource IN ($HA_GC_BILLING_RESOURCES)
        )
        WHEN 'runtime' THEN EXISTS(
          SELECT 1 FROM ha_runtime_outbox
          WHERE created_at < unixepoch() - 14 * 24 * 60 * 60
            AND resource IN ($HA_GC_RUNTIME_RESOURCES)
        )
        ELSE 0
      END
    FROM ha_outbox_gc_channel_state AS state
    ORDER BY CASE state.channel
      WHEN 'control' THEN 0
      WHEN 'billing' THEN 1
      WHEN 'runtime' THEN 2
      ELSE 3
    END;
  " > "$target_path"
}

capture_reconciliation_state() {
  local database_path="$1"
  local target_path="$2"
  local fixture_token_id="testbox-reconciliation-shadow-token"
  local fixture_period_code="testbox-reconciliation-shadow-period"
  local fixture_research_request_id="testbox-reconciliation-research-request"
  local projection_p95=0
  if [[ "$(sqlite3 "$database_path" "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'upstream_reconciliation_projection_state');")" == "1" ]]; then
    projection_p95="$(sqlite3 "$database_path" "SELECT COALESCE(transaction_p95_ms, 0) FROM upstream_reconciliation_projection_state WHERE id = 'local';")"
  fi
  sqlite3 -tabs "$database_path" "
    SELECT
      COALESCE(SUM(completed_generation >= work_generation), 0),
      COALESCE(SUM(last_outcome = 'settled'), 0),
      COALESCE(SUM(last_outcome = 'no_adjustment'), 0),
      COALESCE(SUM(last_outcome = 'observed'), 0),
      $projection_p95,
      COALESCE((SELECT COUNT(*) FROM billing_reconciliation_adjustments), 0),
      COALESCE((SELECT SUM(delta_credits) FROM billing_reconciliation_adjustments), 0),
      COALESCE((
        SELECT completed_generation >= work_generation
               AND last_outcome = 'no_adjustment'
          FROM upstream_reconciliation_work
         WHERE token_id = '$fixture_token_id'
           AND period_code = '$fixture_period_code'
      ), 0),
      COALESCE((SELECT COUNT(*) FROM upstream_reconciliation_research WHERE terminal_at IS NOT NULL), 0),
      COALESCE((SELECT COUNT(*) FROM upstream_reconciliation_research WHERE terminal_at IS NULL), 0),
      COALESCE((
        SELECT terminal_at IS NOT NULL
          FROM upstream_reconciliation_research
         WHERE request_id = '$fixture_research_request_id'
      ), 0)
    FROM upstream_reconciliation_work;
  " > "$target_path"
}

prepare_reconciliation_fixture() {
  local database_path="$1"
  sqlite3 "$database_path" <<'SQL'
-- The comparison network has only the local stub upstream. A copied production
-- subscription can otherwise restore persisted proxy endpoints and consume the
-- reconciliation request budget before the request reaches that stub.
UPDATE forward_proxy_settings
   SET proxy_urls_json = '[]',
       subscription_urls_json = '[]',
       insert_direct = 1,
       egress_socks5_enabled = 0,
       egress_socks5_url = '',
       updated_at = unixepoch();
UPDATE meta
   SET value = '0'
 WHERE key IN (
   'upstream_reconciliation_pressure_streak_v1',
   'upstream_reconciliation_backoff_level_v1',
   'upstream_reconciliation_backoff_until_v1',
   'upstream_reconciliation_local_pressure_streak_v1',
   'upstream_reconciliation_local_backoff_level_v1',
   'upstream_reconciliation_local_backoff_until_v1'
 );
UPDATE upstream_reconciliation_work
   SET next_attempt_at = 0
 WHERE completed_generation < work_generation;
UPDATE upstream_reconciliation_settlements
   SET next_attempt_at = 0
 WHERE status IN ('pending', 'waiting', 'rate_limited');
UPDATE scheduled_jobs
   SET available_at = 0
 WHERE job_type = 'upstream_reconciliation'
   AND status = 'queued';

-- Historical work on a production snapshot may reference retired keys or
-- malformed legacy periods. Keep it for projection coverage, but inject one
-- deterministic current-shape shadow work item so both variants prove a
-- terminal compare outcome against the isolated stub (which returns usage=0).
-- It has its own test-only key. Mark it low-quota-depleted in the cloned
-- month so API rebalance excludes it from foreground selection.
-- Reconciliation fetches its persisted secret directly and remains able to exercise both
-- fixture requests.
-- This is clone-only test data and never reaches the source snapshot.
INSERT OR IGNORE INTO api_keys (
  id, api_key, status, created_at, status_changed_at, last_used_at, deleted_at
) VALUES (
  'testbox-reconciliation-shadow-key', 'tvly-reconciliation-fixture-key', 'active',
  unixepoch(), unixepoch(), 0, NULL
);
UPDATE api_keys
   SET status = 'active', deleted_at = NULL, status_changed_at = unixepoch()
 WHERE api_key = 'tvly-reconciliation-fixture-key';
INSERT INTO api_key_low_quota_depletions (
  key_id, month_start, threshold, quota_remaining, created_at
) VALUES (
  (SELECT id FROM api_keys WHERE api_key = 'tvly-reconciliation-fixture-key'),
  unixepoch('now', 'start of month'), 1, 0, unixepoch()
)
ON CONFLICT(key_id, month_start) DO UPDATE SET
  threshold = excluded.threshold,
  quota_remaining = excluded.quota_remaining,
  created_at = excluded.created_at;
INSERT INTO meta (key, value) VALUES
  ('upstream_project_id_mode_v1', 'accessToken'),
  ('api_rebalance_enabled_v1', '1'),
  ('rebalance_mcp_enabled_v1', '1'),
  ('upstream_precise_reconciliation_enabled_v1', '0')
ON CONFLICT(key) DO UPDATE SET value = excluded.value;
INSERT INTO upstream_reconciliation_usage (
  token_id, key_id, period_code, project_id, billing_subject, settlement_mode,
  period_start, period_end, request_count, first_used_at, last_used_at, updated_at
) VALUES (
  'testbox-reconciliation-shadow-token',
  (SELECT id FROM api_keys WHERE api_key = 'tvly-reconciliation-fixture-key'),
  'testbox-reconciliation-shadow-period',
  'testbox-reconciliation-shadow-project',
  'token:testbox-reconciliation-shadow-token', 'shadow',
  -- Keep the clone-only fixture at the newest legal reconciliation boundary.
  -- It must be eligible immediately, yet never compete with rows whose period
  -- has not completed its required 600-second observation window.
  unixepoch() - 1800, unixepoch() - 600, 1,
  unixepoch() - 1800, unixepoch() - 600, unixepoch() - 600
)
ON CONFLICT(token_id, key_id, period_code) DO UPDATE SET
  project_id = excluded.project_id,
  billing_subject = excluded.billing_subject,
  settlement_mode = excluded.settlement_mode,
  period_start = excluded.period_start,
  period_end = excluded.period_end,
  request_count = excluded.request_count,
  first_used_at = excluded.first_used_at,
  last_used_at = excluded.last_used_at,
  updated_at = excluded.updated_at;
INSERT INTO upstream_reconciliation_usage (
  token_id, key_id, period_code, project_id, billing_subject, settlement_mode,
  period_start, period_end, request_count, first_used_at, last_used_at, updated_at
) VALUES (
  'testbox-reconciliation-research-token',
  (SELECT id FROM api_keys WHERE api_key = 'tvly-reconciliation-fixture-key'),
  'testbox-reconciliation-research-period',
  'testbox-reconciliation-research-project',
  'token:testbox-reconciliation-research-token', 'shadow',
  unixepoch() - 1800, unixepoch() - 601, 1,
  unixepoch() - 1800, unixepoch() - 601, unixepoch() - 601
)
ON CONFLICT(token_id, key_id, period_code) DO UPDATE SET
  project_id = excluded.project_id,
  billing_subject = excluded.billing_subject,
  settlement_mode = excluded.settlement_mode,
  period_start = excluded.period_start,
  period_end = excluded.period_end,
  request_count = excluded.request_count,
  first_used_at = excluded.first_used_at,
  last_used_at = excluded.last_used_at,
  updated_at = excluded.updated_at;
INSERT INTO upstream_reconciliation_research (
  request_id, token_id, key_id, period_code, created_at, terminal_at,
  last_polled_at, next_poll_at, poll_attempt_count, last_poll_outcome,
  last_poll_error_kind, updated_at
) VALUES (
  'testbox-reconciliation-research-request',
  'testbox-reconciliation-research-token',
  (SELECT id FROM api_keys WHERE api_key = 'tvly-reconciliation-fixture-key'),
  'testbox-reconciliation-research-period', unixepoch() - 601, NULL,
  NULL, -1, 0, NULL, NULL, unixepoch() - 601
)
ON CONFLICT(request_id) DO UPDATE SET
  token_id = excluded.token_id,
  key_id = excluded.key_id,
  period_code = excluded.period_code,
  created_at = excluded.created_at,
  terminal_at = NULL,
  last_polled_at = NULL,
  next_poll_at = -1,
  poll_attempt_count = 0,
  last_poll_outcome = NULL,
  last_poll_error_kind = NULL,
  updated_at = excluded.updated_at;
INSERT INTO scheduled_jobs (
  job_type, trigger_source, key_id, status, attempt, queued_at, available_at,
  started_at, finished_at
)
SELECT
  'upstream_reconciliation_research_drain', 'auto', NULL, 'queued', 1,
  unixepoch(), 0, NULL, NULL
WHERE NOT EXISTS (
  SELECT 1
    FROM scheduled_jobs
   WHERE job_type = 'upstream_reconciliation_research_drain'
     AND status IN ('queued', 'running')
);
DELETE FROM api_key_transient_backoffs
 WHERE key_id = (SELECT id FROM api_keys WHERE api_key = 'tvly-reconciliation-fixture-key')
   AND scope = 'period_reconciliation';
UPDATE upstream_reconciliation_research_scan_state
   SET cursor_next_poll_at = -1,
       cursor_key_id = '',
       cursor_request_id = '',
       updated_at = 0
 WHERE id = 'local';
SQL

  # Older baselines predate the controller table. When the copied schema
  # already has it, reset its clone-only state before app startup; otherwise
  # the persisted legacy switch above produces compare mode during migration.
  if [[ "$(sqlite3 "$database_path" "
    SELECT EXISTS(
      SELECT 1 FROM sqlite_master
       WHERE type = 'table' AND name = 'upstream_reconciliation_control_state'
    );
  ")" == "1" ]]; then
    sqlite3 "$database_path" <<'SQL'
UPDATE upstream_reconciliation_control_state
   SET mode = 'compare', activation_period_code = NULL,
       activation_period_start = NULL, legacy_active = 0,
       paused_reason = NULL, transitioned_at = unixepoch()
 WHERE id = 'local';
SQL
  fi

  local transport_isolated
  transport_isolated="$(sqlite3 "$database_path" "
    SELECT CASE WHEN COUNT(*) = 1
                       AND COALESCE(SUM(COALESCE(json_array_length(proxy_urls_json), 0)), 0) = 0
                       AND COALESCE(SUM(COALESCE(json_array_length(subscription_urls_json), 0)), 0) = 0
                       AND COALESCE(MIN(insert_direct), 0) = 1
                       AND COALESCE(MAX(egress_socks5_enabled), 0) = 0
                  THEN 1 ELSE 0 END
      FROM forward_proxy_settings;
  ")"
  [[ "$transport_isolated" == "1" ]] || {
    echo "snapshot forward-proxy transport isolation failed" >&2
    exit 3
  }

  local reconciliation_fixture_ready
  reconciliation_fixture_ready="$(sqlite3 "$database_path" "
    SELECT CASE WHEN EXISTS (
      SELECT 1
        FROM upstream_reconciliation_usage AS usage
        JOIN upstream_reconciliation_work AS work
          ON work.token_id = usage.token_id
         AND work.period_code = usage.period_code
       WHERE usage.token_id = 'testbox-reconciliation-shadow-token'
         AND usage.period_code = 'testbox-reconciliation-shadow-period'
         AND usage.key_id = (SELECT id FROM api_keys WHERE api_key = 'tvly-reconciliation-fixture-key')
         AND usage.settlement_mode = 'shadow'
         AND work.completed_generation < work.work_generation
    ) AND EXISTS (
      SELECT 1 FROM upstream_reconciliation_research
       WHERE request_id = 'testbox-reconciliation-research-request'
         AND terminal_at IS NULL AND next_poll_at = -1
    ) AND NOT EXISTS (
      SELECT 1 FROM api_key_transient_backoffs
       WHERE key_id = (SELECT id FROM api_keys WHERE api_key = 'tvly-reconciliation-fixture-key')
         AND scope = 'period_reconciliation'
    ) AND EXISTS (
      SELECT 1 FROM api_key_low_quota_depletions
       WHERE key_id = (SELECT id FROM api_keys WHERE api_key = 'tvly-reconciliation-fixture-key')
         AND month_start = unixepoch('now', 'start of month')
    )
      THEN 1 ELSE 0 END;
  ")"
  [[ "$reconciliation_fixture_ready" == "1" ]] || {
    echo "snapshot reconciliation fixture preparation failed" >&2
    exit 3
  }
}

normalize_baseline_schema_ledger() {
  local database_path="$1"
  local repo="$2"
  local baseline_schema_max_version
  baseline_schema_max_version="$(awk '
    $1 == "const" && $2 ~ /_VERSION:$/ && $3 == "i64" && $4 == "=" {
      version = $5
      sub(/;$/, "", version)
      if (version ~ /^[0-9]+$/ && version + 0 > max_version + 0) {
        max_version = version
      }
    }
    END { print max_version }
  ' "$repo/src/store/key_store_schema_migrations.rs")"
  [[ "$baseline_schema_max_version" =~ ^[0-9]+$ ]] || {
    echo "baseline schema migration version could not be determined" >&2
    exit 3
  }

  # The live snapshot may contain ledger records newer than an historical
  # baseline can validate. Keep the physical schema and business data intact,
  # but remove clone-only future ledger records so the baseline can adopt it.
  sqlite3 "$database_path" \
    "DELETE FROM schema_migrations WHERE version > $baseline_schema_max_version;"
}

trap 'cleanup_compose; cleanup_app_image' EXIT
mkdir -p "$ARTIFACTS_DIR" "$WORK_DIR"

write_compose() {
  local repo="$1"
  local data_dir="$2"
  local artifact_dir="$3"
  local dockerfile="$repo/tests/ha/Dockerfile.performance-recovery.app"
  local runner_uid runner_gid

  # Historical baselines predate this Dockerfile input allowlist. The test harness
  # owns the temporary baseline checkout, so normalize only that build context.
  if ! grep -qx '!rust-toolchain.toml' "$repo/.dockerignore"; then
    printf '\n!rust-toolchain.toml\n' >> "$repo/.dockerignore"
  fi

  runner_uid="$(id -u)"
  runner_gid="$(id -g)"
  sed "s|^FROM rust:1.91-bookworm AS builder$|FROM $TESTBOX_RUST_BASE_IMAGE AS builder|" \
    "$repo/tests/ha/Dockerfile.app" > "$dockerfile"
  grep -qx "FROM $TESTBOX_RUST_BASE_IMAGE AS builder" "$dockerfile" || {
    echo "unexpected testbox app Dockerfile base image" >&2
    exit 2
  }
  cat > "$WORK_DIR/compose.yml" <<EOF
services:
  upstream:
    image: python:3.12-alpine
    command: ["python", "/work/mock_upstream.py", "--bind", "0.0.0.0", "--port", "9001"]
    volumes:
      - $CANDIDATE_REPO/tests/performance_recovery:/work:ro
    networks: [recovery]
    cap_drop: [ALL]
    cap_add: [CHOWN, DAC_OVERRIDE, FSETID, FOWNER, MKNOD, NET_RAW, SETGID, SETUID, SETPCAP, NET_BIND_SERVICE, SYS_CHROOT, KILL, AUDIT_WRITE]
  app:
    build:
      context: $repo
      dockerfile: tests/ha/Dockerfile.performance-recovery.app
    environment:
      TAVILY_API_KEYS: tvly-load-key,tvly-reconciliation-fixture-key
      TAVILY_UPSTREAM: http://upstream:9001
      TAVILY_USAGE_BASE: http://upstream:9001
      PROXY_DB_PATH: /srv/app/data/tavily_proxy.db
      PROXY_BIND: 0.0.0.0
      PROXY_PORT: "8787"
      DEV_OPEN_ADMIN: "true"
      ADMIN_AUTH_FORWARD_ENABLED: "false"
      HA_MODE: single
      NODE_ID: snapshot-comparison
      XRAY_BINARY: /bin/true
      RUST_LOG: "warn,tavily_hikari=info,tavily_hikari::store::sqlite_runtime=debug,tavily_hikari::server::schedulers=debug"
    volumes:
      - $data_dir:/srv/app/data
    user: "$runner_uid:$runner_gid"
    networks: [recovery]
    cap_drop: [ALL]
    cap_add: [CHOWN, DAC_OVERRIDE, FSETID, FOWNER, MKNOD, NET_RAW, SETGID, SETUID, SETPCAP, NET_BIND_SERVICE, SYS_CHROOT, KILL, AUDIT_WRITE]
  load:
    image: python:3.12-alpine
    volumes:
      - $CANDIDATE_REPO/tests/performance_recovery:/work:ro
      - $artifact_dir:/artifacts
    user: "$runner_uid:$runner_gid"
    networks: [recovery]
    cap_drop: [ALL]
    cap_add: [CHOWN, DAC_OVERRIDE, FSETID, FOWNER, MKNOD, NET_RAW, SETGID, SETUID, SETPCAP, NET_BIND_SERVICE, SYS_CHROOT, KILL, AUDIT_WRITE]
networks:
  recovery:
    internal: true
EOF
}

wait_for_dashboard_readiness() {
  local artifact_dir="$1"
  local health_status dashboard_status
  local deadline=$((SECONDS + 300))
  while (( SECONDS < deadline )); do
    # A production-shaped snapshot can retain an Xray configuration. The test
    # deliberately replaces Xray with /bin/true, making the strict /health
    # readiness endpoint return 503 even though the HTTP server and SQLite
    # startup both completed. The comparison needs listener readiness here;
    # strict readiness remains observable in the captured status code.
    health_status="$(compose exec -T app sh -c 'curl -sS --max-time 1 -o /dev/null -w "%{http_code}" http://127.0.0.1:8787/health' 2>/dev/null || true)"
    if [[ "$health_status" =~ ^[1-5][0-9][0-9]$ ]]; then
      printf '%s\n' "$health_status" > "$artifact_dir/startup_health_status.txt"
    fi

    # The comparison measures Dashboard traffic. Do not begin that workload
    # until the snapshot's initial overview build has completed successfully.
    dashboard_status="$(compose exec -T app sh -c 'curl -sS --max-time 10 -o /dev/null -w "%{http_code}" http://127.0.0.1:8787/api/dashboard/overview' 2>/dev/null || true)"
    if [[ "$dashboard_status" == "200" ]]; then
      printf '%s\n' "$dashboard_status" > "$artifact_dir/startup_dashboard_status.txt"
      return 0
    fi
    printf '%s\n' "${dashboard_status:-unreachable}" > "$artifact_dir/startup_dashboard_status.txt"
    sleep 1
  done
  compose logs --no-color > "$artifact_dir/startup_failure.log" 2>&1 || true
  echo "Dashboard overview did not become ready" >&2
  return 1
}

wait_for_http_listener() {
  local artifact_dir="$1"
  local health_status
  local deadline=$((SECONDS + 300))
  while (( SECONDS < deadline )); do
    health_status="$(compose exec -T app sh -c 'curl -sS --max-time 1 -o /dev/null -w "%{http_code}" http://127.0.0.1:8787/health' 2>/dev/null || true)"
    if [[ "$health_status" =~ ^[1-5][0-9][0-9]$ ]]; then
      printf '%s\n' "$health_status" > "$artifact_dir/startup_health_status.txt"
      return 0
    fi
    sleep 1
  done
  compose logs --no-color > "$artifact_dir/startup_failure.log" 2>&1 || true
  echo "application HTTP listener did not become reachable" >&2
  return 1
}

capture_final_workload_snapshot() {
  local artifact_dir="$1"
  # Runtime workload windows emit at most once per 60 seconds. Give the final
  # capture one full interval plus a bounded scheduler cushion.
  local deadline=$((SECONDS + 75))
  local initial_snapshot_count
  local current_snapshot_count
  initial_snapshot_count="$(compose logs --no-color app 2>/dev/null | grep -c "sqlite_workload_window" || true)"
  while (( SECONDS < deadline )); do
    compose exec -T app sh -c \
      'curl -fsS --max-time 5 -o /dev/null http://127.0.0.1:8787/api/dashboard/overview' \
      >/dev/null 2>&1 || true
    current_snapshot_count="$(compose logs --no-color app 2>/dev/null | grep -c "sqlite_workload_window" || true)"
    if (( current_snapshot_count > initial_snapshot_count )); then
      return 0
    fi
    sleep 1
  done
  compose logs --no-color > "$artifact_dir/final_snapshot_failure.log" 2>&1 || true
  echo "final SQLite workload snapshot did not arrive" >&2
  return 1
}

sample_memory() {
  local target="$1"
  while compose ps -q app >/dev/null 2>&1 && [[ -n "$(compose ps -q app)" ]]; do
    compose exec -T app sh -c '
      printf "sample_at=%s " "$(date +%s)"
      awk '\''
        /^VmRSS:/ { printf "rss_kib=%s ", $2 }
        /^RssAnon:/ { printf "rss_anon_kib=%s ", $2 }
        /^RssFile:/ { printf "rss_file_kib=%s ", $2 }
        /^VmSwap:/ { printf "vm_swap_kib=%s ", $2 }
      '\'' /proc/1/status
      awk '\''/^(anon|file|swap) / { printf "cgroup_%s_bytes=%s ", $1, $2 }'\'' \
        /sys/fs/cgroup/memory.stat
      printf "memory_current_bytes=%s\\n" "$(cat /sys/fs/cgroup/memory.current)"
    ' \
      >> "$target" 2>/dev/null || true
    sleep 5
  done
}

run_variant() {
  local name="$1"
  local repo="$2"
  local variant_dir="$WORK_DIR/$name"
  local artifact_dir="$ARTIFACTS_DIR/$name"
  local load_pid restart_pid rss_pid load_start_deadline
  remove_variant_data "$variant_dir"
  rm -rf -- "$artifact_dir"
  mkdir -p "$variant_dir" "$artifact_dir"
  expand_variant_data "$variant_dir"
  # The production snapshot may be captured during a transient local/remote
  # backoff. Reset retry timing and add one zero-delta shadow fixture only in
  # the isolated copy, so both variants exercise identical durable work while
  # the copied production billing truth remains unchanged.
  prepare_reconciliation_fixture "$variant_dir/tavily_proxy.db"
  if [[ "$name" == "baseline" ]]; then
    normalize_baseline_schema_ledger "$variant_dir/tavily_proxy.db" "$repo"
  fi
  write_compose "$repo" "$variant_dir" "$artifact_dir"
  # The testbox is deliberately isolated from production services. Reusing its
  # locked base-image cache keeps a transient registry failure out of the
  # baseline/candidate comparison.
  compose build app
  compose up -d app upstream
  if [[ "$name" == "baseline" ]]; then
    # Historical baselines may be below the dashboard cold-build coverage
    # contract. Let the comparator classify that red baseline instead of
    # discarding the entire comparison before its measured load starts.
    wait_for_http_listener "$artifact_dir"
  else
    wait_for_dashboard_readiness "$artifact_dir"
  fi
  capture_ha_gc_state "$variant_dir/tavily_proxy.db" "$artifact_dir/ha_gc_before.tsv"
  capture_reconciliation_state "$variant_dir/tavily_proxy.db" "$artifact_dir/reconciliation_before.tsv"
  sample_memory "$artifact_dir/memory_samples.txt" &
  rss_pid=$!
  (
    if ! compose run --rm load python /work/load.py \
      --duration-secs "$DURATION_SECS" \
      --output "/artifacts/load.json"; then
      compose logs --no-color >&2 || true
      exit 1
    fi
  ) &
  load_pid=$!
  load_start_deadline=$((SECONDS + 240))
  while [[ ! -f "$artifact_dir/load_started_at" ]]; do
    if ! kill -0 "$load_pid" 2>/dev/null; then
      if ! wait "$load_pid"; then
        compose logs --no-color >&2 || true
        return 1
      fi
      echo "load exited before measured traffic started" >&2
      compose logs --no-color >&2 || true
      return 1
    fi
    if (( SECONDS >= load_start_deadline )); then
      kill "$load_pid" 2>/dev/null || true
      wait "$load_pid" 2>/dev/null || true
      compose logs --no-color >&2 || true
      echo "load did not reach its measured traffic window" >&2
      return 1
    fi
    sleep 1
  done
  (
    sleep $((DURATION_SECS / 2))
    compose restart app
  ) &
  restart_pid=$!
  wait "$load_pid"
  wait "$restart_pid"
  capture_final_workload_snapshot "$artifact_dir"
  kill "$rss_pid" 2>/dev/null || true
  wait "$rss_pid" 2>/dev/null || true
  capture_ha_gc_state "$variant_dir/tavily_proxy.db" "$artifact_dir/ha_gc_after.tsv"
  capture_reconciliation_state "$variant_dir/tavily_proxy.db" "$artifact_dir/reconciliation_after.tsv"
  compose logs --no-color > "$artifact_dir/compose.log" 2>&1 || true
  python3 - "$name" "$artifact_dir" <<'PY'
import json
import os
import pathlib
import re
import statistics
import sys
from datetime import datetime, timezone

name = sys.argv[1]
artifact_dir = pathlib.Path(sys.argv[2])
CANDIDATE_SHA = os.environ.get("CANDIDATE_SHA", "unknown")
BASELINE_SHA = os.environ.get("BASELINE_SHA", "unknown")
load = json.loads((artifact_dir / "load.json").read_text())

def read_ha_gc_state(path):
    state = {}
    for line in path.read_text().splitlines():
        channel, deleted, oldest_age, has_debt = line.split("\t")
        state[channel] = {
            "totalDeletedRows": int(deleted),
            "oldestDeletableAgeSecs": int(oldest_age),
            "hasRetentionDebt": bool(int(has_debt)),
        }
    return state

ha_gc_before = read_ha_gc_state(artifact_dir / "ha_gc_before.tsv")
ha_gc_after = read_ha_gc_state(artifact_dir / "ha_gc_after.tsv")

def read_reconciliation_state(path):
    values = [int(value) for value in path.read_text().strip().split("\t")]
    keys = (
        "terminal", "settled", "noAdjustment", "observed",
        "projectionTransactionP95Ms", "billingAdjustmentCount", "billingAdjustmentSum",
        "fixtureNoAdjustmentTerminal", "researchTerminal", "researchPending",
        "fixtureResearchTerminal",
    )
    return dict(zip(keys, values, strict=True))

reconciliation_before = read_reconciliation_state(artifact_dir / "reconciliation_before.tsv")
reconciliation_after = read_reconciliation_state(artifact_dir / "reconciliation_after.tsv")
samples = []
for line in (artifact_dir / "memory_samples.txt").read_text().splitlines():
    sample = {}
    for token in line.split():
        key, separator, value = token.partition("=")
        if separator and value.isdigit():
            sample[key] = int(value)
    if sample:
        samples.append(sample)

def p95(key):
    values = sorted(sample[key] for sample in samples if key in sample)
    if not values:
        return None
    return values[min(len(values) - 1, int(len(values) * 0.95))]

def percentile(values):
    ordered = sorted(values)
    if not ordered:
        return None
    return ordered[min(len(ordered) - 1, int(len(ordered) * 0.95))]

logs = (artifact_dir / "compose.log").read_text(errors="replace")
sqlite_lock_markers = (
    "database is locked",
    "database table is locked",
    "database schema is locked",
    "database is busy",
)

load_started_at = load.get("startedAt")

def line_is_in_load_window(line):
    if not isinstance(load_started_at, (int, float)):
        return True
    timestamp_match = re.search(r'"timestamp":"([^"]+)"', line)
    if timestamp_match is None:
        return True
    try:
        event_at = datetime.fromisoformat(
            timestamp_match.group(1).replace("Z", "+00:00")
        )
    except ValueError:
        return True
    if event_at.tzinfo is None:
        event_at = event_at.replace(tzinfo=timezone.utc)
    return event_at.timestamp() >= load_started_at

# Credential creation happens before load.startedAt and has its own bounded
# bootstrap retry. Keep startup contention out of the measured request-path
# SQLite gates while retaining timestamped lines from the actual load window.
measured_log_lines = [line for line in logs.splitlines() if line_is_in_load_window(line)]

# A retry or typed admission deferral is evidence of recoverable contention,
# not a foreground request failure. Count each structured log line once so the
# message/err duplication in tracing fields cannot inflate the rate. Keep
# final lock errors separate: the candidate must never return one, while
# successful retries have a small absolute budget under the deliberate
# concurrent writer workload.
sqlite_lock_lines = [
    line
    for line in measured_log_lines
    if any(marker in line for marker in sqlite_lock_markers)
]

def structured_field(line, field, value):
    return (
        f'"{field}":"{value}"' in line
        or f'"{field}":{value}' in line
        or f"{field}={value}" in line
    )

def structured_value(line, field):
    escaped_field = re.escape(field)
    json_match = re.search(
        rf'"{escaped_field}":(?:"((?:\\.|[^"\\])*)"|(-?[0-9]+(?:\.[0-9]+)?))',
        line,
    )
    if json_match:
        return json_match.group(1) or json_match.group(2)
    text_match = re.search(rf'(?:^|[\s,]){escaped_field}=([^\s]+)', line)
    return text_match.group(1).strip('"') if text_match else None

def parse_int(value):
    if value in (None, "none", "unknown"):
        return None
    try:
        return int(value)
    except ValueError:
        return None

def parse_maintenance_snapshot(raw):
    if not raw or ",classes=" not in raw:
        return None
    active_raw, classes_raw = raw.split(",classes=", 1)
    snapshot = {"active": active_raw.removeprefix("active="), "classes": {}}
    for class_raw in classes_raw.split("|"):
        class_name, separator, fields_raw = class_raw.partition(":")
        if not separator or not class_name:
            continue
        fields = {}
        for field_raw in fields_raw.split(","):
            field, separator, value = field_raw.partition("=")
            if separator:
                fields[field] = value
        snapshot["classes"][class_name] = {
            "pendingAgeMs": parse_int(fields.get("pending_age_ms")),
            "admissions": parse_int(fields.get("admissions")) or 0,
            "completed": parse_int(fields.get("completed")) or 0,
            "maxWaitMs": parse_int(fields.get("max_wait_ms")) or 0,
            "stale": parse_int(fields.get("stale")) or 0,
        }
    return snapshot

maintenance_snapshots = []
maintenance_admission_events = []
foreground_hold_p95_samples = []
for line in measured_log_lines:
    snapshot = parse_maintenance_snapshot(structured_value(line, "maintenance_admission"))
    if snapshot:
        maintenance_snapshots.append(snapshot)
    if structured_field(line, "event", "sqlite_maintenance_admitted"):
        maintenance_admission_events.append({
            "class": structured_value(line, "maintenance_class"),
            "pendingAgeMs": parse_int(structured_value(line, "pending_age_ms")),
        })
    top_operations = structured_value(line, "top_operations")
    if top_operations:
        for operation in top_operations.split(";"):
            if operation.startswith("foreground_work/"):
                hold_match = re.search(r"hold_p95_ms=(\d+)", operation)
                if hold_match:
                    foreground_hold_p95_samples.append(int(hold_match.group(1)))

maintenance_classes = {}
pending_age_samples = []
wait_samples = []
pending_class_counts = []
for snapshot in maintenance_snapshots:
    pending_count = 0
    for class_name, values in snapshot["classes"].items():
        state = maintenance_classes.setdefault(
            class_name,
            {
                "admittedSlices": 0,
                "pendingAgeSamples": [],
                "waitSamples": [],
                "maxWaitMs": 0,
                "eventCount": 0,
            },
        )
        state["admittedSlices"] = max(state["admittedSlices"], values["admissions"])
        state["maxWaitMs"] = max(state["maxWaitMs"], values["maxWaitMs"])
        if values["pendingAgeMs"] is not None:
            pending_count += 1
            pending_age_samples.append(values["pendingAgeMs"])
            wait_samples.append(values["pendingAgeMs"])
            state["pendingAgeSamples"].append(values["pendingAgeMs"])
            state["waitSamples"].append(values["pendingAgeMs"])
    pending_class_counts.append(pending_count)
for event in maintenance_admission_events:
    class_name = event["class"]
    if not class_name:
        continue
    state = maintenance_classes.setdefault(
        class_name,
        {
            "admittedSlices": 0,
            "pendingAgeSamples": [],
            "waitSamples": [],
            "maxWaitMs": 0,
            "eventCount": 0,
        },
    )
    state["eventCount"] += 1
    state["admittedSlices"] = max(state["admittedSlices"], state["eventCount"])
    if event["pendingAgeMs"] is not None:
        state["maxWaitMs"] = max(state["maxWaitMs"], event["pendingAgeMs"])
        wait_samples.append(event["pendingAgeMs"])
        state["waitSamples"].append(event["pendingAgeMs"])

maintenance_class_summary = {}
for class_name, state in sorted(maintenance_classes.items()):
    class_waits = state["waitSamples"]
    maintenance_class_summary[class_name] = {
        "admittedSlices": state["admittedSlices"],
        "eventCount": state["eventCount"],
        "pendingAgeSampleCount": len(state["pendingAgeSamples"]),
        "waitSampleCount": len(class_waits),
        "maxWaitMs": max(state["maxWaitMs"], max(class_waits, default=0)),
        "p95WaitMs": percentile(class_waits),
    }

maintenance_admission = {
    "snapshotCount": len(maintenance_snapshots),
    "admissionEventCount": len(maintenance_admission_events),
    "classes": maintenance_class_summary,
    "pendingAgeSampleCount": len(pending_age_samples),
    "pendingAgeP95Ms": percentile(pending_age_samples),
    "maxPendingAgeMs": max(pending_age_samples) if pending_age_samples else 0,
    "waitSampleCount": len(wait_samples),
    "waitP95Ms": percentile(wait_samples),
    "maxWaitMs": max(
        (class_metrics["maxWaitMs"] for class_metrics in maintenance_class_summary.values()),
        default=0,
    ),
    "maxPendingClassCount": max(pending_class_counts) if pending_class_counts else 0,
    "finalPendingAgeMaxMs": max(
        (
            values["pendingAgeMs"]
            for values in maintenance_snapshots[-1]["classes"].values()
            if values["pendingAgeMs"] is not None
        ),
        default=0,
    )
    if maintenance_snapshots
    else 0,
}

sqlite_transient_lock_retries = sum(
    structured_field(line, "event", "sqlite_transient_write_retry")
    or structured_field(line, "event", "sqlite_transient_read_retry")
    or ("transient sqlite error" in line and "attempt=" in line)
    for line in sqlite_lock_lines
)
sqlite_typed_lock_deferrals = sum(
    any(
        structured_field(line, "defer_reason", reason)
        for reason in ("sqlite_contention", "sqlite_busy")
    )
    or (
        structured_field(line, "event", "research_sweep_deferred")
        and structured_field(line, "reason", "local_pressure")
    )
    for line in sqlite_lock_lines
)
sqlite_final_lock_errors = (
    len(sqlite_lock_lines) - sqlite_transient_lock_retries - sqlite_typed_lock_deferrals
)
sqlite_pool_timeout_errors = sum(
    structured_field(line, "workload_class", "foreground_work")
    and structured_value(line, "operation") != "foreground_job_trigger"
    and (
        structured_field(line, "pool_timeout", "true")
        or "PoolTimedOut" in line
        or "pool timed out" in line.lower()
    )
    for line in measured_log_lines
)

def lane_5xx(lane):
    return sum(
        count
        for key, count in load["statuses"].items()
        if key.startswith(f"{lane}:") and int(key.split(":", 1)[1]) >= 500
    )

def lane_transport_errors(load_summary, lane):
    return sum(
        count
        for key, count in load_summary.get("errors", {}).items()
        if key.startswith(f"{lane}:")
    )

def lane_http_rejections(load_summary, lane, accepted_status):
    return sum(
        count
        for key, count in load_summary["statuses"].items()
        if key.startswith(f"{lane}:")
        and int(key.split(":", 1)[1]) != accepted_status
    )

summary = {
    "variant": name,
    "sourceSha": CANDIDATE_SHA if name == "candidate" else BASELINE_SHA,
    "load": load,
    "rssP95KiB": p95("rss_kib"),
    "foregroundTransactionP95Ms": percentile(foreground_hold_p95_samples),
    "memoryP95": {
        key: p95(key)
        for key in (
            "rss_anon_kib",
            "rss_file_kib",
            "vm_swap_kib",
            "cgroup_anon_bytes",
            "cgroup_file_bytes",
            "cgroup_swap_bytes",
            "memory_current_bytes",
        )
    },
    "sqliteTransientLockRetries": sqlite_transient_lock_retries,
    "sqliteTypedLockDeferrals": sqlite_typed_lock_deferrals,
    "sqliteFinalLockErrors": sqlite_final_lock_errors,
    "sqlitePoolTimeoutErrors": sqlite_pool_timeout_errors,
    "maintenanceAdmission": maintenance_admission,
    "nestedTransactionErrors": sum(
        "cannot start a transaction within a transaction" in line
        for line in measured_log_lines
    ),
    "reconciliationProjectionDiscarded": sum(
        structured_field(line, "event", "sqlite_transaction_connection_discarded")
        and structured_field(line, "operation", "reconciliation_projection")
        for line in measured_log_lines
    ),
    "foregroundHttp5xx": lane_5xx("business"),
    "dashboardHttp5xx": lane_5xx("dashboard"),
    "maintenanceHttp5xx": lane_5xx("ha_gc_trigger"),
    "dashboardTransportErrors": lane_transport_errors(load, "dashboard"),
    "maintenanceTransportErrors": lane_transport_errors(load, "ha_gc_trigger"),
    "dashboardHttpRejections": lane_http_rejections(load, "dashboard", 200),
    "maintenanceHttpRejections": lane_http_rejections(load, "ha_gc_trigger", 202),
    "haGc": {
        "before": ha_gc_before,
        "after": ha_gc_after,
        "deletedRowsDelta": {
            channel: ha_gc_after[channel]["totalDeletedRows"] - values["totalDeletedRows"]
            for channel, values in ha_gc_before.items()
        },
    },
    "reconciliation": {
        "before": reconciliation_before,
        "after": reconciliation_after,
        "terminalDelta": reconciliation_after["terminal"] - reconciliation_before["terminal"],
        "researchTerminalDelta": (
            reconciliation_after["researchTerminal"] - reconciliation_before["researchTerminal"]
        ),
        "researchPendingDelta": (
            reconciliation_after["researchPending"] - reconciliation_before["researchPending"]
        ),
    },
}
(artifact_dir / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
PY
  cleanup_compose
  cleanup_app_image
  remove_variant_data "$variant_dir"
}

test "$(sha256sum "$CORE_COMPRESSED_DB" | awk '{print $1}')" = "$CORE_COMPRESSED_SNAPSHOT_SHA256"
test "$(sha256sum "$SIDECAR_COMPRESSED_DB" | awk '{print $1}')" = "$SIDECAR_COMPRESSED_SNAPSHOT_SHA256"

run_variant baseline "$BASELINE_REPO"
run_variant candidate "$CANDIDATE_REPO"

# Keep failed-gate diagnosis bounded and non-sensitive. Stable event and
# operation fields identify the writer owner without exposing SQL or payloads;
# the caller removes the exact run directory after printing this tail.
for variant in baseline candidate; do
    echo "--- ${variant} summary ---"
    cat "$ARTIFACTS_DIR/$variant/summary.json"
    echo "--- ${variant} startup events ---"
    grep -E 'startup_|schema_migration|schema migration|database is locked|nested transaction|sqlite_workload_window|slow_slice|ha_outbox_gc|upstream_reconciliation|maintenance_dequeue' \
        "$ARTIFACTS_DIR/$variant/compose.log" | tail -160 || true
done

python3 - "$ARTIFACTS_DIR" <<'PY'
import json
import math
import pathlib
import sys

artifacts = pathlib.Path(sys.argv[1])
baseline = json.loads((artifacts / "baseline" / "summary.json").read_text())
candidate = json.loads((artifacts / "candidate" / "summary.json").read_text())

# Linux process RSS and sub-15ms HTTP timings are sampled across a controlled
# restart. Keep raw values in the receipt, but do not turn allocator or
# scheduler jitter into a false regression when the absolute SLO has ample
# headroom. These margins are calibrated by a same-SHA A/B run.
DASHBOARD_P95_NOISE_FLOOR_MS = 15.0
RSS_P95_NOISE_BAND_KIB = 40 * 1024
MAINTENANCE_FRESHNESS_BOUND_MS = 60_000
CONTROLLED_RESTART_HTTP_5XX_RATE_PERCENT = 5
PRODUCTION_ACCEPTANCE_MIN_DURATION_SECS = 600

def p95(summary):
    return summary["load"]["dashboardP95Ms"]

def lane_transport_errors(load_summary, lane):
    return sum(
        count
        for key, count in load_summary.get("errors", {}).items()
        if key.startswith(f"{lane}:")
    )

def lane_http_responses(load_summary, lane):
    return sum(
        count
        for key, count in load_summary["statuses"].items()
        if key.startswith(f"{lane}:")
    )

def lane_http_rejections(load_summary, lane, accepted_status):
    return sum(
        count
        for key, count in load_summary["statuses"].items()
        if key.startswith(f"{lane}:")
        and int(key.split(":", 1)[1]) != accepted_status
    )

def expected_periodic_attempts(duration_secs, interval_secs, initial_delay_secs=0.0):
    if initial_delay_secs >= duration_secs:
        return 0
    return int((duration_secs - initial_delay_secs - 1e-9) // interval_secs) + 1

def expected_maintenance_attempts(load_summary):
    scheduled = expected_periodic_attempts(
        load_summary["trafficDurationSecs"],
        60.0,
        17.0,
    )
    recovery_tail_attempt = int(load_summary.get("recoveryTailSecs", 0) > 0)
    return scheduled + recovery_tail_attempt

def is_diagnostic_duration(duration_secs):
    return duration_secs < PRODUCTION_ACCEPTANCE_MIN_DURATION_SECS

def acceptance_status_for_duration(duration_secs):
    return "diagnostic" if is_diagnostic_duration(duration_secs) else "passed"

def assert_not_worse(metric, base, cand, absolute_floor=None, additive_tolerance=0):
    if base is None or cand is None:
        raise SystemExit(f"missing {metric} sample")
    threshold = max(base * 1.10, absolute_floor or 0) + additive_tolerance
    if cand > threshold:
        raise SystemExit(
            f"candidate {metric} regressed: baseline={base}, candidate={cand}, threshold={threshold}"
        )

baseline_business_attempts = baseline["load"].get("businessAttempts", 0)
baseline_business_responses = (
    baseline["load"]["statuses"].get("business:200", 0)
    + baseline["load"]["statuses"].get("business:429", 0)
)
candidate_business_responses = (
    candidate["load"]["statuses"].get("business:200", 0)
    + candidate["load"]["statuses"].get("business:429", 0)
)
if baseline["load"]["durationSecs"] != candidate["load"]["durationSecs"]:
    raise SystemExit("baseline and candidate duration windows must match")
diagnostic = is_diagnostic_duration(baseline["load"]["durationSecs"])
baseline_business_minimum = (
    baseline["load"]["trafficDurationSecs"]
    * baseline["load"].get("businessClients", 0)
    * (0.10 if diagnostic else 0.30)
)
baseline_application_business_minimum = max(20, baseline_business_minimum / 2)
def dashboard_coverage_is_complete(load_summary):
    expected = load_summary.get("dashboardExpectedAttempts", 0)
    minimum = math.ceil(expected * 0.95)
    attempts = load_summary.get("dashboardAttempts", 0)
    successes = load_summary["statuses"].get("dashboard:200", 0)
    expected_by_client = load_summary.get("dashboardExpectedAttemptsByClient", {})
    successes_by_client = load_summary.get("dashboardSuccessesByClient", {})
    return (
        expected > 0
        and attempts >= minimum
        and successes >= minimum
        and all(
            successes_by_client.get(client, 0) >= max(1, expected_count - 1)
            for client, expected_count in expected_by_client.items()
        )
    )


baseline_dashboard_red = not diagnostic and not dashboard_coverage_is_complete(baseline["load"])
baseline_business_red = not diagnostic and (
    baseline_business_attempts < baseline_business_minimum
    or baseline_business_responses < baseline_application_business_minimum
)

for summary in (baseline, candidate):
    statuses = summary["load"]["statuses"]
    events = summary["load"]["events"]
    dashboard_clients = summary["load"].get("dashboardClients")
    dashboard_interval_secs = summary["load"].get("dashboardIntervalSecs")
    dashboard_attempts = summary["load"].get("dashboardAttempts")
    traffic_duration_secs = summary["load"].get("trafficDurationSecs")
    recovery_tail_secs = summary["load"].get("recoveryTailSecs")
    diagnostic = is_diagnostic_duration(summary["load"]["durationSecs"])
    if dashboard_clients != 20 or dashboard_interval_secs != 60.0:
        raise SystemExit(f"unexpected dashboard load shape for {summary['variant']}")
    expected_recovery_tail_secs = 0 if diagnostic else 60
    if traffic_duration_secs is None or recovery_tail_secs != expected_recovery_tail_secs:
        raise SystemExit(f"missing quiet GC recovery tail for {summary['variant']}")
    # A short diagnostic intentionally restarts the app halfway through. The
    # ten-minute production-shape gate below retains p95 and sustained
    # coverage comparisons.
    dashboard_coverage = 0.20
    business_clients = summary["load"].get("businessClients")
    business_interval_secs = summary["load"].get("businessIntervalSecs")
    if business_clients != 5 or business_interval_secs != 1.0:
        raise SystemExit(f"unexpected business load shape for {summary['variant']}")
    dashboard_expected_attempts = summary["load"].get("dashboardExpectedAttempts", 0)
    dashboard_minimum = (
        dashboard_expected_attempts * 0.95
        if not diagnostic
        else summary["load"]["durationSecs"]
        * dashboard_clients
        / dashboard_interval_secs
        * dashboard_coverage
    )
    business_minimum = traffic_duration_secs * business_clients * (0.10 if diagnostic else 0.30)
    if dashboard_attempts is None or dashboard_attempts < dashboard_minimum:
        raise SystemExit(f"insufficient dashboard coverage for {summary['variant']}")
    # A diagnostic only proves startup/recovery wiring. A production-shaped run
    # must keep 95% of its scheduled Dashboard traffic and at least all but one
    # sample from each staggered client, independent of baseline quality.
    required_dashboard_successes = (
        2
        if diagnostic or summary["variant"] == "baseline"
        else math.ceil(dashboard_expected_attempts * 0.95)
    )
    if not diagnostic and summary["variant"] == "candidate":
        if not dashboard_coverage_is_complete(summary["load"]):
            raise SystemExit(f"insufficient per-client dashboard response coverage for {summary['variant']}")
    elif not diagnostic and summary["variant"] == "baseline" and not baseline_dashboard_red:
        if not dashboard_coverage_is_complete(summary["load"]):
            raise SystemExit(f"insufficient per-client dashboard response coverage for {summary['variant']}")
    if statuses.get("dashboard:200", 0) < required_dashboard_successes:
        raise SystemExit(f"insufficient dashboard response coverage for {summary['variant']}")
    if statuses.get("sse:200", 0) < 20:
        raise SystemExit(f"insufficient SSE coverage for {summary['variant']}")
    business_attempts = summary["load"].get("businessAttempts", 0)
    required_business_attempts = (
        business_minimum
        if summary["variant"] == "candidate" or not baseline_business_red
        else 1
    )
    if business_attempts < required_business_attempts:
        raise SystemExit(f"insufficient business coverage for {summary['variant']}")
    # 429 is an application response, not a transport failure. Count it as
    # reached application code while keeping HTTP 5xx and connection errors
    # visible in the separate regression gates below.
    application_business_responses = (
        statuses.get("business:200", 0) + statuses.get("business:429", 0)
    )
    application_business_minimum = max(20, business_minimum / 2)
    # Both variants perform a controlled restart halfway through the run and
    # their open-loop clients can complete a different number of attempts.
    # Compare application-response ratios instead of absolute response counts;
    # the absolute workload, 5xx, lock, and latency gates below remain strict.
    baseline_business_response_ratio = (
        baseline_business_responses / baseline_business_attempts
        if baseline_business_attempts
        else 0.0
    )
    candidate_response_ratio_floor = max(0.0, baseline_business_response_ratio - 0.05)
    required_application_business_responses = (
        max(
            application_business_minimum,
            int(business_attempts * candidate_response_ratio_floor),
        )
        if summary["variant"] == "candidate"
        else (application_business_minimum if not baseline_business_red else 1)
    )
    if application_business_responses < required_application_business_responses:
        raise SystemExit(f"insufficient application business coverage for {summary['variant']}")
    if events.get("ha_export_interrupted", 0) < 1:
        raise SystemExit(f"missing HA export interruption for {summary['variant']}")

candidate_gc = candidate["haGc"]
for channel, before in candidate_gc["before"].items():
    if before["hasRetentionDebt"] and candidate_gc["deletedRowsDelta"].get(channel, 0) <= 0:
        raise SystemExit(f"candidate HA GC did not advance the debt-bearing {channel} channel")

baseline_red = baseline_dashboard_red or baseline_business_red
if baseline_red:
    reasons = []
    if baseline_dashboard_red:
        reasons.append("dashboard")
    if baseline_business_red:
        reasons.append("business")
    print(
        "baseline is already below required production-shaped coverage for "
        f"{','.join(reasons)}; candidate retains its absolute coverage gates",
        file=sys.stderr,
    )
if not diagnostic:
    if baseline_dashboard_red:
        print(
            "Dashboard p95 comparison is non-comparable because the baseline did not "
            "meet per-client measured-window coverage; retaining both raw values",
            file=sys.stderr,
        )
    else:
        assert_not_worse(
            "dashboard p95",
            p95(baseline),
            p95(candidate),
            absolute_floor=DASHBOARD_P95_NOISE_FLOOR_MS,
        )
    if baseline_business_red:
        print(
            "RSS P95 comparison is non-comparable because the baseline did not "
            "process the required business workload; retaining both raw values",
            file=sys.stderr,
        )
    else:
        assert_not_worse(
            "RSS P95",
            baseline["rssP95KiB"],
            candidate["rssP95KiB"],
            additive_tolerance=RSS_P95_NOISE_BAND_KIB,
        )
    # Successful retries remain observable, but only final lock errors fail
    # the foreground contract. A zero-retry baseline cannot be used as a
    # multiplicative threshold once the candidate independently advances GC.
    candidate_lock_limit = max(5, (candidate_business_responses + 199) // 200)
    if candidate["sqliteTransientLockRetries"] > candidate_lock_limit:
        raise SystemExit(
            "candidate transient SQLite lock rate exceeded 0.5%: "
            f"retries={candidate['sqliteTransientLockRetries']}, "
            f"responses={candidate_business_responses}, "
            f"limit={candidate_lock_limit}"
        )
if candidate["sqliteFinalLockErrors"]:
    raise SystemExit(
        "candidate emitted a final SQLite lock error: "
        f"errors={candidate['sqliteFinalLockErrors']}"
    )
if candidate["sourceSha"] == "unknown":
    raise SystemExit("candidate source SHA was not supplied to the comparison")

# Both variants restart halfway through the run. Compare raw counts in the
# receipt, but bound candidate failures by rate so phase alignment does not
# turn one controlled restart response into a false regression.
if not diagnostic:
    for lane, metric in (
        ("dashboard", "dashboardHttp5xx"),
        ("maintenance", "maintenanceHttp5xx"),
    ):
        status_lane = "ha_gc_trigger" if lane == "maintenance" else lane
        accepted_status = 202 if lane == "maintenance" else 200
        candidate_http_responses = lane_http_responses(candidate["load"], status_lane)
        candidate_http_rejections = lane_http_rejections(
            candidate["load"], status_lane, accepted_status
        )
        candidate_transport_errors = lane_transport_errors(candidate["load"], status_lane)
        candidate_attempts = candidate_http_responses + candidate_transport_errors
        if candidate_http_responses <= 0:
            raise SystemExit(
                f"candidate {lane} lane produced no HTTP responses: "
                f"transport_errors={candidate_transport_errors}"
            )
        if lane == "maintenance":
            expected_attempts = expected_maintenance_attempts(candidate["load"])
            if candidate_attempts < expected_attempts:
                raise SystemExit(
                    "candidate maintenance lane did not complete its scheduled attempts: "
                    f"attempts={candidate_attempts}, expected={expected_attempts}"
                )
        candidate_failures = candidate_http_rejections + candidate_transport_errors
        if (
            candidate_failures * 100
            > candidate_attempts * CONTROLLED_RESTART_HTTP_5XX_RATE_PERCENT
        ):
            raise SystemExit(
                f"candidate {lane} HTTP/transport failure rate exceeded the "
                f"{CONTROLLED_RESTART_HTTP_5XX_RATE_PERCENT}% controlled-restart allowance: "
                f"accepted_status={accepted_status}, baseline_5xx={baseline[metric]}, "
                f"candidate_5xx={candidate[metric]}, candidate_http_rejections={candidate_http_rejections}, "
                f"candidate_transport_errors={candidate_transport_errors}, "
                f"attempts={candidate_attempts}, failures={candidate_failures}"
            )

baseline_request_path_errors = (
    baseline["sqliteFinalLockErrors"] + baseline["sqlitePoolTimeoutErrors"]
)
candidate_request_path_errors = (
    candidate["sqliteFinalLockErrors"] + candidate["sqlitePoolTimeoutErrors"]
)
if candidate_request_path_errors > baseline_request_path_errors:
    raise SystemExit(
        "candidate request-path SQLite lock/pool errors increased: "
        f"baseline={baseline_request_path_errors}, candidate={candidate_request_path_errors}"
    )
if baseline_request_path_errors and candidate_request_path_errors * 2 > baseline_request_path_errors:
    raise SystemExit(
        "candidate request-path SQLite lock/pool errors did not fall by at least 50%: "
        f"baseline={baseline_request_path_errors}, candidate={candidate_request_path_errors}"
    )

candidate_admission = candidate["maintenanceAdmission"]
if candidate_admission["snapshotCount"] <= 0:
    raise SystemExit("candidate emitted no maintenance admission snapshots")
if candidate_admission["pendingAgeSampleCount"] <= 0:
    raise SystemExit("candidate emitted no maintenance admission freshness telemetry")
for class_name, class_metrics in candidate_admission["classes"].items():
    exercised = (
        class_metrics["admittedSlices"] > 0
        or class_metrics["pendingAgeSampleCount"] > 0
        or class_metrics["eventCount"] > 0
    )
    if exercised and class_metrics["maxWaitMs"] >= MAINTENANCE_FRESHNESS_BOUND_MS:
        raise SystemExit(
            "candidate maintenance class exceeded the 60-second fairness bound: "
            f"class={class_name}, wait_ms={class_metrics['maxWaitMs']}"
        )
if candidate_admission["finalPendingAgeMaxMs"] >= MAINTENANCE_FRESHNESS_BOUND_MS:
    raise SystemExit(
        "candidate oldest pending maintenance work reached the 60-second quiet-tail bound: "
        f"age_ms={candidate_admission['finalPendingAgeMaxMs']}"
    )
baseline_pending_p95 = baseline["maintenanceAdmission"]["pendingAgeP95Ms"]
candidate_pending_p95 = candidate_admission["pendingAgeP95Ms"]
if baseline_pending_p95 is not None:
    candidate_pending_p95_value = candidate_pending_p95 or 0
    if candidate_pending_p95_value > baseline_pending_p95 * 0.80:
        raise SystemExit(
            "candidate maintenance pending-age p95 did not improve by at least 20%: "
            f"baseline={baseline_pending_p95}, candidate={candidate_pending_p95_value}"
        )
else:
    candidate_age_bound = max(
        candidate_admission["maxPendingAgeMs"],
        candidate_admission["maxWaitMs"],
        candidate_admission["finalPendingAgeMaxMs"],
    )
    if candidate_age_bound >= MAINTENANCE_FRESHNESS_BOUND_MS:
        raise SystemExit(
            "baseline had no pending-age sample, but candidate maintenance freshness exceeded "
            f"the 60-second bound: age_ms={candidate_age_bound}"
        )
if not diagnostic:
    assert_not_worse(
        "foreground transaction hold p95",
        baseline["foregroundTransactionP95Ms"],
        candidate["foregroundTransactionP95Ms"],
    )
if candidate["foregroundHttp5xx"]:
    raise SystemExit(
        "candidate introduced foreground HTTP 5xx: "
        f"candidate={candidate['foregroundHttp5xx']}"
    )
if candidate["nestedTransactionErrors"]:
    raise SystemExit("candidate emitted a nested transaction error")
if candidate["reconciliationProjectionDiscarded"]:
    raise SystemExit("candidate discarded a reconciliation projection transaction connection")
if candidate["reconciliation"]["terminalDelta"] <= 0:
    raise SystemExit("candidate reconciliation produced no terminal outcome")
if not candidate["reconciliation"]["after"]["fixtureNoAdjustmentTerminal"]:
    raise SystemExit("candidate did not complete the deterministic shadow reconciliation fixture")
if candidate["reconciliation"]["researchTerminalDelta"] <= 0:
    raise SystemExit("candidate Research drain produced no terminal outcome")
if candidate["reconciliation"]["researchPendingDelta"] > 0:
    raise SystemExit("candidate Research pending backlog grew during the comparison")
if not candidate["reconciliation"]["after"]["fixtureResearchTerminal"]:
    raise SystemExit("candidate did not complete the deterministic Research drain fixture")
projection_p95 = candidate["reconciliation"]["after"]["projectionTransactionP95Ms"]
if projection_p95 <= 0 or projection_p95 >= 100:
    raise SystemExit(
        f"candidate reconciliation projection transaction p95 is not proven below 100ms: {projection_p95}"
    )
for billing_field in ("billingAdjustmentCount", "billingAdjustmentSum"):
    baseline_value = baseline["reconciliation"]["after"][billing_field]
    candidate_value = candidate["reconciliation"]["after"][billing_field]
    if candidate_value != baseline_value:
        raise SystemExit(
            f"candidate billing truth differs for {billing_field}: "
            f"baseline={baseline_value}, candidate={candidate_value}"
        )

# Short runs exercise startup/recovery wiring only; keep their receipt
# explicitly non-accepting because the production-shape comparison gates are skipped.
acceptance_status = acceptance_status_for_duration(baseline["load"]["durationSecs"])
result = {
    "baseline": baseline,
    "candidate": candidate,
    "baseline_dashboard_red": baseline_dashboard_red,
    "baseline_business_red": baseline_business_red,
    "rss_p95_comparable": not baseline_business_red,
    "sqlite_lock_rate_comparable": False,
    "baseline_transient_sqlite_lock_rate": (
        baseline["sqliteTransientLockRetries"] / baseline_business_responses
        if baseline_business_responses
        else None
    ),
    "candidate_transient_sqlite_lock_rate": (
        candidate["sqliteTransientLockRetries"] / candidate_business_responses
        if candidate_business_responses
        else None
    ),
    "empiricalAcceptance": {
        "status": acceptance_status,
        "candidateSha": candidate["sourceSha"],
        "baselineSha": baseline["sourceSha"],
        "foregroundTransactionP95Ms": {
            "baseline": baseline["foregroundTransactionP95Ms"],
            "candidate": candidate["foregroundTransactionP95Ms"],
        },
        "requestPathSqliteErrors": {
            "baseline": baseline_request_path_errors,
            "candidate": candidate_request_path_errors,
        },
        "maintenanceFreshness": {
            "baselinePendingAgeP95Ms": baseline_pending_p95,
            "candidatePendingAgeP95Ms": candidate_pending_p95,
            "candidateFinalPendingAgeMaxMs": candidate_admission["finalPendingAgeMaxMs"],
            "candidateMaxWaitMs": candidate_admission["maxWaitMs"],
        },
    },
    "result": (
        "diagnostic"
        if diagnostic
        else ("passed_with_baseline_red" if baseline_red else "passed")
    ),
}
(artifacts / "comparison.json").write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
print(json.dumps(result, sort_keys=True))
PY
