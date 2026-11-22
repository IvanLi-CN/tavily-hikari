#!/usr/bin/env bash
set -Eeuo pipefail

# Release gate for the SQLite -> PostgreSQL migrator image (REQ-PG-005, REQ-PG-013).
#
# Consumed migrator CLI contract:
#   s2pg snapshot --sqlite <db-path> --out <snapshot-path>
#   s2pg load     --snapshot <snapshot-path> --database-url <postgres-url> [--reset]
#   s2pg verify   --snapshot <snapshot-path> --database-url <postgres-url> --report <path>
#   s2pg report   --report <path>
#
# `load` applies the embedded baseline DDL to an empty target before copying rows, so a
# small fixture snapshot must leave the full 123-table baseline in the target database.
# The gate then requires `load` to refuse the now non-empty target, `verify` to accept the
# same snapshot, and `verify` to reject a target whose rows were mutated afterwards.

LOCAL_MIGRATOR_IMAGE="${LOCAL_MIGRATOR_IMAGE:-}"
if [[ -z "${LOCAL_MIGRATOR_IMAGE}" ]]; then
  echo "LOCAL_MIGRATOR_IMAGE is required" >&2
  exit 2
fi

PG_URL="${S2PG_TEST_PG_URL:-postgres://s2pg_smoke:s2pg_smoke@127.0.0.1:5432/s2pg_smoke}"
PSQL_IMAGE="${S2PG_PSQL_IMAGE:-postgres:18}"
EXPECTED_BASELINE_TABLES="${S2PG_EXPECTED_BASELINE_TABLES:-123}"

TEMP_ROOT="${RUNNER_TEMP:-$(mktemp -d)}"
TEMP_ROOT_IS_EPHEMERAL=0
if [[ -z "${RUNNER_TEMP:-}" ]]; then
  TEMP_ROOT_IS_EPHEMERAL=1
fi

SMOKE_ID="$$"
WORK_DIR="${TEMP_ROOT}/release-s2pg-smoke-${SMOKE_ID}"
FIXTURE_DIR="${WORK_DIR}/fixture"
SQLITE_PATH="${FIXTURE_DIR}/sample.db"
SNAPSHOT_DIR="${WORK_DIR}/snapshot"
SNAPSHOT_PATH="${SNAPSHOT_DIR}/sample.snapshot.db"
REPORTS_DIR="${WORK_DIR}/reports"
VERIFY_REPORT="${REPORTS_DIR}/verification.json"
MISMATCH_REPORT="${REPORTS_DIR}/verification-mismatch.json"
PSQL_CONTAINER="release-s2pg-smoke-psql-${SMOKE_ID}"
MIGRATOR_CONTAINER="release-s2pg-smoke-migrator-${SMOKE_ID}"
DIAGNOSTICS_EMITTED=0

read -r PG_USER PG_PASSWORD PG_HOST PG_PORT PG_DB < <(python3 - "${PG_URL}" <<'PY'
from __future__ import annotations

import sys
from urllib.parse import unquote, urlsplit

parsed = urlsplit(sys.argv[1])
print(
    unquote(parsed.username or ""),
    unquote(parsed.password or ""),
    parsed.hostname or "127.0.0.1",
    parsed.port or 5432,
    (parsed.path or "/").lstrip("/") or "postgres",
)
PY
)

print_section() {
  local title="$1"
  echo "::group::${title}"
}

end_section() {
  echo "::endgroup::"
}

run_psql() {
  local sql="$1"
  docker run --rm \
    --name "${PSQL_CONTAINER}" \
    --network host \
    -e "PGPASSWORD=${PG_PASSWORD}" \
    "${PSQL_IMAGE}" \
    psql -h "${PG_HOST}" -p "${PG_PORT}" -U "${PG_USER}" -d "${PG_DB}" \
    -v ON_ERROR_STOP=1 -tAc "${sql}"
}

trim_whitespace() {
  local value="$1"
  printf '%s' "${value//[[:space:]]/}"
}

count_matching_tables() {
  trim_whitespace "$(run_psql "SELECT count(*) FROM pg_tables WHERE schemaname IN ('core', 'observability')")"
}

count_table_rows() {
  local schema_and_table="$1"
  trim_whitespace "$(run_psql "SELECT count(*) FROM ${schema_and_table}")"
}

run_migrator() {
  docker run --rm \
    --name "${MIGRATOR_CONTAINER}" \
    --network host \
    -v "${WORK_DIR}:/work" \
    "${LOCAL_MIGRATOR_IMAGE}" "$@"
}

dump_migrator_help() {
  print_section "s2pg CLI surface"
  docker run --rm "${LOCAL_MIGRATOR_IMAGE}" --help || true
  docker run --rm "${LOCAL_MIGRATOR_IMAGE}" snapshot --help || true
  docker run --rm "${LOCAL_MIGRATOR_IMAGE}" load --help || true
  docker run --rm "${LOCAL_MIGRATOR_IMAGE}" verify --help || true
  end_section
}

dump_pg_state() {
  print_section "target database state"
  echo "pg_url=${PG_URL}"
  run_psql "SELECT schemaname, tablename FROM pg_tables WHERE schemaname IN ('core', 'observability') ORDER BY 1, 2" || true
  end_section
}

dump_docker_state() {
  print_section "release smoke docker state"
  docker ps -a || true
  end_section
}

dump_diagnostics() {
  if [[ "${DIAGNOSTICS_EMITTED}" == "1" ]]; then
    return 0
  fi
  DIAGNOSTICS_EMITTED=1
  dump_migrator_help || true
  dump_pg_state || true
  dump_docker_state || true
}

cleanup() {
  set +e
  docker rm -f "${MIGRATOR_CONTAINER}" >/dev/null 2>&1 || true
  docker rm -f "${PSQL_CONTAINER}" >/dev/null 2>&1 || true
  if [[ "${TEMP_ROOT_IS_EPHEMERAL}" == "1" ]]; then
    rm -rf "${TEMP_ROOT}" >/dev/null 2>&1 || true
  fi
}

on_error() {
  local exit_code="$?"
  local line_no="$1"
  local command="$2"
  echo "s2pg release smoke failed at line ${line_no}: ${command} (exit=${exit_code})" >&2
  dump_diagnostics
  exit "${exit_code}"
}

trap 'on_error "${LINENO}" "${BASH_COMMAND}"' ERR
trap cleanup EXIT

echo "s2pg release smoke config: image=${LOCAL_MIGRATOR_IMAGE} pg=${PG_HOST}:${PG_PORT}/${PG_DB} expected_tables=${EXPECTED_BASELINE_TABLES}"

docker image inspect "${LOCAL_MIGRATOR_IMAGE}" >/dev/null

mkdir -p "${FIXTURE_DIR}" "${SNAPSHOT_DIR}" "${REPORTS_DIR}"
python3 - "${SQLITE_PATH}" <<'PY'
from __future__ import annotations

import sqlite3
import sys

connection = sqlite3.connect(sys.argv[1])
try:
    connection.executescript(
        """
        CREATE TABLE meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE auth_token_quota (
            token_id TEXT PRIMARY KEY,
            month_start INTEGER NOT NULL,
            month_count INTEGER NOT NULL
        );
        INSERT INTO meta (key, value) VALUES ('schema_version', 'release-smoke');
        INSERT INTO meta (key, value) VALUES ('release_smoke_payload', 'bytea-probe');
        INSERT INTO auth_token_quota (token_id, month_start, month_count)
            VALUES ('thsmoketoken0001', 20454, 7);
        INSERT INTO auth_token_quota (token_id, month_start, month_count)
            VALUES ('thsmoketoken0002', 20454, 42);
        """
    )
    connection.commit()
finally:
    connection.close()
PY

print_section "snapshot the fixture database"
run_migrator snapshot --sqlite /work/fixture/sample.db --out /work/snapshot/sample.snapshot.db
if [[ ! -s "${SNAPSHOT_PATH}" ]]; then
  echo "s2pg snapshot did not write ${SNAPSHOT_PATH}" >&2
  exit 1
fi
echo "snapshot written: $(wc -c < "${SNAPSHOT_PATH}") bytes"
end_section

print_section "reset target database"
run_psql "DROP SCHEMA IF EXISTS core CASCADE" >/dev/null
run_psql "DROP SCHEMA IF EXISTS observability CASCADE" >/dev/null
remaining_tables="$(count_matching_tables)"
if [[ "${remaining_tables}" != "0" ]]; then
  echo "target database still holds ${remaining_tables} tables after reset" >&2
  exit 1
fi
echo "target database reset to zero tables"
end_section

print_section "load baseline plus fixture rows into PostgreSQL"
run_migrator load \
  --snapshot /work/snapshot/sample.snapshot.db \
  --database-url "${PG_URL}"

baseline_tables="$(count_matching_tables)"
if [[ "${baseline_tables}" != "${EXPECTED_BASELINE_TABLES}" ]]; then
  echo "expected ${EXPECTED_BASELINE_TABLES} baseline tables in ${PG_DB}, got ${baseline_tables}" >&2
  exit 1
fi
echo "baseline tables present: ${baseline_tables}"

expected_meta_rows=2
meta_rows="$(count_table_rows core.meta)"
if [[ "${meta_rows}" != "${expected_meta_rows}" ]]; then
  echo "expected ${expected_meta_rows} core.meta rows after load, got ${meta_rows}" >&2
  exit 1
fi

expected_quota_rows=2
quota_rows="$(count_table_rows core.auth_token_quota)"
if [[ "${quota_rows}" != "${expected_quota_rows}" ]]; then
  echo "expected ${expected_quota_rows} core.auth_token_quota rows after load, got ${quota_rows}" >&2
  exit 1
fi

fixture_meta_rows="$(trim_whitespace "$(run_psql "SELECT count(*) FROM core.meta WHERE key = 'schema_version' AND value = 'release-smoke'")")"
if [[ "${fixture_meta_rows}" != "1" ]]; then
  echo "core.meta does not hold the fixture row after load: ${fixture_meta_rows}" >&2
  exit 1
fi
echo "fixture rows loaded: core.meta=${meta_rows} core.auth_token_quota=${quota_rows}"
end_section

print_section "load must refuse the non-empty target"
if run_migrator load \
  --snapshot /work/snapshot/sample.snapshot.db \
  --database-url "${PG_URL}"; then
  echo "s2pg load accepted a non-empty target" >&2
  exit 1
fi
echo "s2pg load refused the non-empty target"
end_section

print_section "verify snapshot against PostgreSQL"
run_migrator verify \
  --snapshot /work/snapshot/sample.snapshot.db \
  --database-url "${PG_URL}" \
  --report /work/reports/verification.json
if [[ ! -s "${VERIFY_REPORT}" ]]; then
  echo "s2pg verify did not write ${VERIFY_REPORT}" >&2
  exit 1
fi
echo "verification report written: $(wc -c < "${VERIFY_REPORT}") bytes"
end_section

print_section "verify must reject mutated target rows"
run_psql "UPDATE core.meta SET value = 'tampered-by-release-smoke' WHERE key = 'schema_version'" >/dev/null
if run_migrator verify \
  --snapshot /work/snapshot/sample.snapshot.db \
  --database-url "${PG_URL}" \
  --report /work/reports/verification-mismatch.json; then
  echo "s2pg verify accepted a mutated target" >&2
  exit 1
fi
echo "s2pg verify rejected the mutated target"
end_section

echo "s2pg release smoke passed"
