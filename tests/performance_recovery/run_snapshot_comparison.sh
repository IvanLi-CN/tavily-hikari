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
-- This is clone-only test data and never reaches the source snapshot.
INSERT OR IGNORE INTO api_keys (
  id, api_key, status, created_at, status_changed_at, last_used_at, deleted_at
) VALUES (
  'testbox-reconciliation-shadow-key', 'tvly-load-key', 'active',
  unixepoch(), unixepoch(), 0, NULL
);
UPDATE api_keys
   SET status = 'active', deleted_at = NULL, status_changed_at = unixepoch()
 WHERE api_key = 'tvly-load-key';
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
  (SELECT id FROM api_keys WHERE api_key = 'tvly-load-key'),
  'testbox-reconciliation-shadow-period',
  'testbox-reconciliation-shadow-project',
  'token:testbox-reconciliation-shadow-token', 'shadow',
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
INSERT INTO upstream_reconciliation_usage (
  token_id, key_id, period_code, project_id, billing_subject, settlement_mode,
  period_start, period_end, request_count, first_used_at, last_used_at, updated_at
) VALUES (
  'testbox-reconciliation-research-token',
  (SELECT id FROM api_keys WHERE api_key = 'tvly-load-key'),
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
  (SELECT id FROM api_keys WHERE api_key = 'tvly-load-key'),
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
DELETE FROM api_key_transient_backoffs
 WHERE key_id = (SELECT id FROM api_keys WHERE api_key = 'tvly-load-key')
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
         AND usage.key_id = (SELECT id FROM api_keys WHERE api_key = 'tvly-load-key')
         AND usage.settlement_mode = 'shadow'
         AND work.completed_generation < work.work_generation
    ) AND EXISTS (
      SELECT 1 FROM upstream_reconciliation_research
       WHERE request_id = 'testbox-reconciliation-research-request'
         AND terminal_at IS NULL AND next_poll_at = -1
    ) AND NOT EXISTS (
      SELECT 1 FROM api_key_transient_backoffs
       WHERE key_id = (SELECT id FROM api_keys WHERE api_key = 'tvly-load-key')
         AND scope = 'period_reconciliation'
    )
      THEN 1 ELSE 0 END;
  ")"
  [[ "$reconciliation_fixture_ready" == "1" ]] || {
    echo "snapshot reconciliation fixture preparation failed" >&2
    exit 3
  }
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
      TAVILY_API_KEYS: tvly-load-key
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
  local load_pid restart_pid rss_pid
  remove_variant_data "$variant_dir"
  rm -rf -- "$artifact_dir"
  mkdir -p "$variant_dir" "$artifact_dir"
  expand_variant_data "$variant_dir"
  # The production snapshot may be captured during a transient local/remote
  # backoff. Reset retry timing and add one zero-delta shadow fixture only in
  # the isolated copy, so both variants exercise identical durable work while
  # the copied production billing truth remains unchanged.
  prepare_reconciliation_fixture "$variant_dir/tavily_proxy.db"
  write_compose "$repo" "$variant_dir" "$artifact_dir"
  # The testbox is deliberately isolated from production services. Reusing its
  # locked base-image cache keeps a transient registry failure out of the
  # baseline/candidate comparison.
  compose build app
  compose up -d app upstream
  wait_for_dashboard_readiness "$artifact_dir"
  capture_ha_gc_state "$variant_dir/tavily_proxy.db" "$artifact_dir/ha_gc_before.tsv"
  capture_reconciliation_state "$variant_dir/tavily_proxy.db" "$artifact_dir/reconciliation_before.tsv"
  sample_memory "$artifact_dir/memory_samples.txt" &
  rss_pid=$!
  (
    sleep $((DURATION_SECS / 2))
    compose restart app
  ) &
  restart_pid=$!
  (
    if ! compose run --rm load python /work/load.py \
      --duration-secs "$DURATION_SECS" \
      --output "/artifacts/load.json"; then
      compose logs --no-color >&2 || true
      exit 1
    fi
  ) &
  load_pid=$!
  wait "$load_pid"
  wait "$restart_pid"
  kill "$rss_pid" 2>/dev/null || true
  wait "$rss_pid" 2>/dev/null || true
  capture_ha_gc_state "$variant_dir/tavily_proxy.db" "$artifact_dir/ha_gc_after.tsv"
  capture_reconciliation_state "$variant_dir/tavily_proxy.db" "$artifact_dir/reconciliation_after.tsv"
  compose logs --no-color > "$artifact_dir/compose.log" 2>&1 || true
  python3 - "$name" "$artifact_dir" <<'PY'
import json
import pathlib
import statistics
import sys

name = sys.argv[1]
artifact_dir = pathlib.Path(sys.argv[2])
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

logs = (artifact_dir / "compose.log").read_text(errors="replace")
sqlite_lock_markers = (
    "database is locked",
    "database table is locked",
    "database schema is locked",
    "database is busy",
)

# A retry or typed admission deferral is evidence of recoverable contention,
# not a foreground request failure. Count each structured log line once so the
# message/err duplication in tracing fields cannot inflate the rate. Keep
# final lock errors separate: the candidate must never return one, while
# successful retries have a small absolute budget under the deliberate
# concurrent writer workload.
sqlite_lock_lines = [
    line
    for line in logs.splitlines()
    if any(marker in line for marker in sqlite_lock_markers)
]

def structured_field(line, field, value):
    return f'"{field}":"{value}"' in line or f"{field}={value}" in line

sqlite_transient_lock_retries = sum(
    structured_field(line, "event", "sqlite_transient_write_retry")
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

def lane_5xx(lane):
    return sum(
        count
        for key, count in load["statuses"].items()
        if key.startswith(f"{lane}:") and int(key.split(":", 1)[1]) >= 500
    )

summary = {
    "variant": name,
    "load": load,
    "rssP95KiB": p95("rss_kib"),
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
    "nestedTransactionErrors": logs.count("cannot start a transaction within a transaction"),
    "reconciliationProjectionDiscarded": sum(
        structured_field(line, "event", "sqlite_transaction_connection_discarded")
        and structured_field(line, "operation", "reconciliation_projection")
        for line in logs.splitlines()
    ),
    "foregroundHttp5xx": lane_5xx("business"),
    "dashboardHttp5xx": lane_5xx("dashboard"),
    "maintenanceHttp5xx": lane_5xx("ha_gc_trigger"),
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

def p95(summary):
    return summary["load"]["dashboardP95Ms"]

def assert_not_worse(metric, base, cand, absolute_floor=None, additive_tolerance=0):
    if base is None or cand is None:
        raise SystemExit(f"missing {metric} sample")
    threshold = max(base * 1.10, absolute_floor or 0) + additive_tolerance
    if cand > threshold:
        raise SystemExit(
            f"candidate {metric} regressed: baseline={base}, candidate={cand}, threshold={threshold}"
        )

baseline_dashboard_successes = baseline["load"]["statuses"].get("dashboard:200", 0)
baseline_dashboard_clients = baseline["load"].get("dashboardClients", 0)
baseline_business_attempts = baseline["load"].get("businessAttempts", 0)
baseline_business_responses = (
    baseline["load"]["statuses"].get("business:200", 0)
    + baseline["load"]["statuses"].get("business:429", 0)
)
candidate_business_responses = (
    candidate["load"]["statuses"].get("business:200", 0)
    + candidate["load"]["statuses"].get("business:429", 0)
)
diagnostic = baseline["load"]["durationSecs"] <= 120
baseline_business_minimum = (
    baseline["load"]["trafficDurationSecs"]
    * baseline["load"].get("businessClients", 0)
    * (0.10 if diagnostic else 0.30)
)
baseline_application_business_minimum = max(20, baseline_business_minimum / 2)
baseline_dashboard_red = (
    not diagnostic and baseline_dashboard_successes < baseline_dashboard_clients
)
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
    diagnostic = summary["load"]["durationSecs"] <= 120
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
    dashboard_minimum = (
        summary["load"]["durationSecs"] * dashboard_clients / dashboard_interval_secs * dashboard_coverage
    )
    business_minimum = traffic_duration_secs * business_clients * (0.10 if diagnostic else 0.30)
    if dashboard_attempts is None or dashboard_attempts < dashboard_minimum:
        raise SystemExit(f"insufficient dashboard coverage for {summary['variant']}")
    # The 60-second diagnosis contains a halfway restart and production-shaped
    # cold aggregation, so require one successful sample from each tenure. The
    # Ten-minute comparisons tolerate the bounded controlled-restart race
    # below five percent while retaining enough coverage to compare p95 and
    # error rates. The load driver schedules 200 dashboard attempts at this
    # duration, so this still requires at least 190 successful snapshots.
    required_dashboard_successes = (
        2
        if diagnostic or summary["variant"] == "baseline"
        else max(2, (baseline_dashboard_successes * 95 + 99) // 100)
    )
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
    "result": "passed_with_baseline_red" if baseline_red else "passed",
}
(artifacts / "comparison.json").write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
print(json.dumps(result, sort_keys=True))
PY
