#!/usr/bin/env bash
set -Eeuo pipefail

LOCAL_SMOKE_IMAGE="${LOCAL_SMOKE_IMAGE:-}"
if [[ -z "${LOCAL_SMOKE_IMAGE}" ]]; then
  echo "LOCAL_SMOKE_IMAGE is required" >&2
  exit 2
fi

SMOKE_MOCK_BIN="${SMOKE_MOCK_BIN:-./target/debug/mock_tavily}"
SMOKE_MOCK_MODE=""
if [[ -x "${SMOKE_MOCK_BIN}" ]]; then
  SMOKE_MOCK_MODE="mock_tavily"
elif command -v python3 >/dev/null 2>&1; then
  SMOKE_MOCK_MODE="python_stub"
else
  echo "need either executable SMOKE_MOCK_BIN or python3 for a fallback stub mock" >&2
  exit 2
fi

SCENARIO="${SCENARIO:-positive}"
POSITIVE_MIN_HEALTHY_SECS="${POSITIVE_MIN_HEALTHY_SECS:-20}"
POSITIVE_MAX_HEALTHY_SECS="${POSITIVE_MAX_HEALTHY_SECS:-35}"
NEGATIVE_OBSERVE_SECS="${NEGATIVE_OBSERVE_SECS:-35}"

TEMP_ROOT="${RUNNER_TEMP:-$(mktemp -d)}"
TEMP_ROOT_IS_EPHEMERAL=0
if [[ -z "${RUNNER_TEMP:-}" ]]; then
  TEMP_ROOT_IS_EPHEMERAL=1
fi

allocate_port() {
  local bind_host="$1"
  local exclude_csv="${2:-}"
  python3 - "${bind_host}" "${exclude_csv}" <<'PY'
from __future__ import annotations

import socket
import sys

bind_host = sys.argv[1]
exclude = {int(part) for part in sys.argv[2].split(",") if part}
for _ in range(64):
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.bind((bind_host, 0))
    port = sock.getsockname()[1]
    sock.close()
    if port not in exclude:
        print(port)
        break
else:
    raise SystemExit("failed to allocate a free localhost port")
PY
}

MOCK_BIND_HOST="${SMOKE_MOCK_BIND_HOST:-0.0.0.0}"
MOCK_HOST="${SMOKE_MOCK_HOST:-127.0.0.1}"
PROXY_HOST="${SMOKE_PROXY_HOST:-127.0.0.1}"
MOCK_PORT="${SMOKE_MOCK_PORT:-$(allocate_port "${MOCK_BIND_HOST}")}"
PROXY_PORT="${SMOKE_PROXY_PORT:-$(allocate_port "${PROXY_HOST}" "${MOCK_PORT}")}"
MOCK_BASE_URL="http://${MOCK_HOST}:${MOCK_PORT}"
PROXY_BASE_URL="http://${PROXY_HOST}:${PROXY_PORT}"
MOCK_BIND_ADDR="${MOCK_BIND_HOST}:${MOCK_PORT}"
MOCK_LOG="${SMOKE_MOCK_LOG:-${TEMP_ROOT}/mock-tavily-${MOCK_PORT}.log}"
POSITIVE_DATA_DIR="${TEMP_ROOT}/health-positive-${MOCK_PORT}-${PROXY_PORT}"
NEGATIVE_DATA_DIR="${TEMP_ROOT}/health-negative-${MOCK_PORT}-${PROXY_PORT}"
POSITIVE_CONTAINER_NAME="${SMOKE_CONTAINER_NAME_PREFIX:-tavily-hikari-health}-positive-${MOCK_PORT}-${PROXY_PORT}"
NEGATIVE_SEED_CONTAINER_NAME="${SMOKE_CONTAINER_NAME_PREFIX:-tavily-hikari-health}-negative-seed-${MOCK_PORT}-${PROXY_PORT}"
NEGATIVE_CONTAINER_NAME="${SMOKE_CONTAINER_NAME_PREFIX:-tavily-hikari-health}-negative-${MOCK_PORT}-${PROXY_PORT}"
NEGATIVE_SHARE_LINK="${NEGATIVE_SHARE_LINK:-vless://0688fa59-e971-4278-8c03-4b35821a71dc@health-smoke.example.com:443?encryption=none#HealthSmoke}"
MOCK_PID=""

print_section() {
  local title="$1"
  echo "::group::${title}"
}

end_section() {
  echo "::endgroup::"
}

container_exists() {
  local name="$1"
  docker container inspect "${name}" >/dev/null 2>&1
}

container_status() {
  local name="$1"
  docker container inspect --format '{{.State.Status}}' "${name}" 2>/dev/null || true
}

container_health() {
  local name="$1"
  docker container inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}none{{end}}' "${name}" 2>/dev/null || true
}

dump_mock_log() {
  if [[ -f "${MOCK_LOG}" ]]; then
    print_section "mock_tavily log"
    cat "${MOCK_LOG}" || true
    end_section
  fi
}

dump_container_logs() {
  local name="$1"
  if container_exists "${name}"; then
    print_section "container logs: ${name}"
    echo "status=$(container_status "${name}") health=$(container_health "${name}")"
    docker logs "${name}" || true
    end_section
  fi
}

dump_data_dir() {
  local path="$1"
  if [[ -d "${path}" ]]; then
    print_section "data dir: ${path}"
    ls -la "${path}" || true
    find "${path}" -maxdepth 2 -mindepth 1 -print | sort || true
    end_section
  fi
}

cleanup_container() {
  local name="$1"
  if container_exists "${name}"; then
    docker rm -f "${name}" >/dev/null 2>&1 || true
  fi
}

cleanup() {
  set +e
  cleanup_container "${POSITIVE_CONTAINER_NAME}"
  cleanup_container "${NEGATIVE_SEED_CONTAINER_NAME}"
  cleanup_container "${NEGATIVE_CONTAINER_NAME}"
  if [[ -n "${MOCK_PID}" ]]; then
    kill "${MOCK_PID}" >/dev/null 2>&1 || true
    wait "${MOCK_PID}" >/dev/null 2>&1 || true
  fi
  if [[ "${TEMP_ROOT_IS_EPHEMERAL}" == "1" ]]; then
    rm -rf "${TEMP_ROOT}" >/dev/null 2>&1 || true
  fi
}

on_error() {
  local exit_code="$?"
  local line_no="$1"
  local command="$2"
  echo "health smoke failed at line ${line_no}: ${command} (exit=${exit_code})" >&2
  dump_mock_log || true
  dump_container_logs "${POSITIVE_CONTAINER_NAME}" || true
  dump_container_logs "${NEGATIVE_SEED_CONTAINER_NAME}" || true
  dump_container_logs "${NEGATIVE_CONTAINER_NAME}" || true
  dump_data_dir "${POSITIVE_DATA_DIR}" || true
  dump_data_dir "${NEGATIVE_DATA_DIR}" || true
  exit "${exit_code}"
}

trap 'on_error "${LINENO}" "${BASH_COMMAND}"' ERR
trap cleanup EXIT

wait_for_mock_ready() {
  local url="${MOCK_BASE_URL}/admin/state"
  local attempt
  for attempt in {1..50}; do
    if [[ -n "${MOCK_PID}" ]] && ! kill -0 "${MOCK_PID}" >/dev/null 2>&1; then
      echo "mock_tavily exited before becoming ready" >&2
      dump_mock_log
      return 1
    fi
    if curl -fsS --max-time 2 "${url}" >/dev/null; then
      return 0
    fi
    sleep 0.2
  done
  echo "timed out waiting for mock_tavily readiness: ${url}" >&2
  dump_mock_log
  return 1
}

start_mock() {
  if [[ "${SMOKE_MOCK_MODE}" == "mock_tavily" ]]; then
    "${SMOKE_MOCK_BIN}" --bind "${MOCK_BIND_ADDR}" >"${MOCK_LOG}" 2>&1 &
  else
    python3 - "${MOCK_BIND_HOST}" "${MOCK_PORT}" >"${MOCK_LOG}" 2>&1 <<'PY' &
from __future__ import annotations

import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

bind_host = sys.argv[1]
port = int(sys.argv[2])


class Handler(BaseHTTPRequestHandler):
    def _write(self, status: int, payload: dict[str, object]) -> None:
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:  # noqa: N802
        if self.path == "/admin/state":
            self._write(200, {"ok": True})
            return
        self._write(200, {})

    def do_POST(self) -> None:  # noqa: N802
        length = int(self.headers.get("content-length", "0"))
        if length:
            self.rfile.read(length)
        if self.path == "/mcp":
            self._write(200, {"jsonrpc": "2.0", "result": {}})
            return
        self._write(200, {"ok": True})

    def log_message(self, format: str, *args: object) -> None:
        return


ThreadingHTTPServer((bind_host, port), Handler).serve_forever()
PY
  fi
  MOCK_PID=$!
  wait_for_mock_ready
  curl -fsS \
    -X POST \
    "${MOCK_BASE_URL}/admin/keys" \
    -H 'content-type: application/json' \
    -d '{"secret":"tvly-test-key","limit":1000,"remaining":1000}' \
    >/dev/null
}

run_proxy_container() {
  local name="$1"
  local data_dir="$2"
  shift 2
  mkdir -p "${data_dir}"
  docker run -d --rm \
    --name "${name}" \
    --user "$(id -u):$(id -g)" \
    --add-host host.docker.internal:host-gateway \
    -p "${PROXY_HOST}:${PROXY_PORT}:8787" \
    -v "${data_dir}:/srv/app/data" \
    -e TAVILY_API_KEYS=tvly-test-key \
    -e TAVILY_UPSTREAM="http://host.docker.internal:${MOCK_PORT}/mcp" \
    -e TAVILY_USAGE_BASE="http://host.docker.internal:${MOCK_PORT}" \
    -e DEV_OPEN_ADMIN=true \
    -e PROXY_DB_PATH=/srv/app/data/tavily_proxy.db \
    "$@" \
    "${LOCAL_SMOKE_IMAGE}" \
    >/dev/null
}

wait_for_http_ok() {
  local url="$1"
  local timeout_secs="$2"
  local attempt
  for ((attempt = 0; attempt < timeout_secs; attempt++)); do
    if curl -fsS --max-time 2 "${url}" >/dev/null; then
      return 0
    fi
    sleep 1
  done
  echo "timed out waiting for HTTP 200: ${url}" >&2
  return 1
}

seed_negative_runtime_requirements() {
  local db_path="$1"
  python3 - "${db_path}" "${NEGATIVE_SHARE_LINK}" <<'PY'
from __future__ import annotations

import json
import sqlite3
import sys
import time

db_path = sys.argv[1]
share_link = sys.argv[2]
conn = sqlite3.connect(db_path)
try:
    conn.execute(
        """
        UPDATE forward_proxy_settings
        SET proxy_urls_json = ?1,
            subscription_urls_json = '[]',
            insert_direct = 0,
            updated_at = ?2
        WHERE id = 1
        """,
        (json.dumps([share_link]), int(time.time())),
    )
    conn.commit()
finally:
    conn.close()
PY
}

run_positive_scenario() {
  local started_at now elapsed
  local ready_at=""
  local healthy_at=""
  local http_code health_status
  local body_file="${TEMP_ROOT}/positive-health-body.txt"

  run_proxy_container "${POSITIVE_CONTAINER_NAME}" "${POSITIVE_DATA_DIR}"
  started_at="$(date +%s)"

  for _ in {1..45}; do
    now="$(date +%s)"
    elapsed="$((now - started_at))"
    health_status="$(container_health "${POSITIVE_CONTAINER_NAME}")"
    http_code="$(curl -sS --max-time 2 -o "${body_file}" -w '%{http_code}' "${PROXY_BASE_URL}/health" || true)"
    if [[ -z "${ready_at}" && "${http_code}" == "200" ]]; then
      ready_at="${elapsed}"
    fi
    if [[ -z "${healthy_at}" && "${health_status}" == "healthy" ]]; then
      healthy_at="${elapsed}"
    fi
    if [[ -n "${ready_at}" && -n "${healthy_at}" ]]; then
      break
    fi
    sleep 1
  done

  if [[ -z "${ready_at}" ]]; then
    echo "positive scenario never reached /health 200" >&2
    return 1
  fi
  if [[ -z "${healthy_at}" ]]; then
    echo "positive scenario never reached container health=healthy" >&2
    return 1
  fi
  if (( healthy_at < POSITIVE_MIN_HEALTHY_SECS )); then
    echo "container became healthy too early: ${healthy_at}s < ${POSITIVE_MIN_HEALTHY_SECS}s" >&2
    return 1
  fi
  if (( healthy_at > POSITIVE_MAX_HEALTHY_SECS )); then
    echo "container became healthy too late: ${healthy_at}s > ${POSITIVE_MAX_HEALTHY_SECS}s" >&2
    return 1
  fi
  if (( healthy_at < ready_at )); then
    echo "container health became healthy before /health was ready: ready_at=${ready_at}s healthy_at=${healthy_at}s" >&2
    return 1
  fi
  if [[ "$(cat "${body_file}")" != "ok" ]]; then
    echo "expected positive /health body to be ok" >&2
    return 1
  fi

  printf '{"scenario":"positive","readyAtSec":%s,"healthyAtSec":%s}\n' \
    "${ready_at}" "${healthy_at}"
}

run_negative_xray_scenario() {
  local db_path="${NEGATIVE_DATA_DIR}/tavily_proxy.db"
  local started_at now elapsed
  local first_unready_at=""
  local http_code health_status container_state
  local body_file="${TEMP_ROOT}/negative-health-body.txt"

  run_proxy_container "${NEGATIVE_SEED_CONTAINER_NAME}" "${NEGATIVE_DATA_DIR}"
  wait_for_http_ok "${PROXY_BASE_URL}/health" 60
  cleanup_container "${NEGATIVE_SEED_CONTAINER_NAME}"

  if [[ ! -f "${db_path}" ]]; then
    echo "expected seeded database at ${db_path}" >&2
    return 1
  fi
  seed_negative_runtime_requirements "${db_path}"

  run_proxy_container \
    "${NEGATIVE_CONTAINER_NAME}" \
    "${NEGATIVE_DATA_DIR}" \
    -e XRAY_BINARY=/tmp/tavily-hikari-missing-xray
  started_at="$(date +%s)"

  for ((elapsed = 0; elapsed <= NEGATIVE_OBSERVE_SECS; elapsed++)); do
    now="$(date +%s)"
    elapsed="$((now - started_at))"
    health_status="$(container_health "${NEGATIVE_CONTAINER_NAME}")"
    container_state="$(container_status "${NEGATIVE_CONTAINER_NAME}")"
    http_code="$(curl -sS --max-time 2 -o "${body_file}" -w '%{http_code}' "${PROXY_BASE_URL}/health" || true)"

    if [[ "${container_state}" != "running" ]]; then
      echo "negative xray scenario container exited unexpectedly: ${container_state}" >&2
      return 1
    fi
    if [[ "${health_status}" == "healthy" ]]; then
      echo "negative xray scenario became healthy unexpectedly at ${elapsed}s" >&2
      return 1
    fi
    if [[ "${http_code}" == "200" ]]; then
      echo "negative xray scenario returned 200 unexpectedly at ${elapsed}s" >&2
      return 1
    fi
    if [[ "${http_code}" == "503" && "$(cat "${body_file}")" == "xray not ready" && -z "${first_unready_at}" ]]; then
      first_unready_at="${elapsed}"
    fi

    sleep 1
  done

  if [[ -z "${first_unready_at}" ]]; then
    echo "negative xray scenario never surfaced /health 503 xray not ready" >&2
    return 1
  fi

  printf '{"scenario":"negative-xray","firstUnreadyAtSec":%s,"observedSec":%s}\n' \
    "${first_unready_at}" "${NEGATIVE_OBSERVE_SECS}"
}

echo "health contract smoke config: scenario=${SCENARIO} mock=${MOCK_BIND_ADDR} proxy=${PROXY_BASE_URL} image=${LOCAL_SMOKE_IMAGE}"

start_mock

case "${SCENARIO}" in
  positive)
    run_positive_scenario
    ;;
  negative-xray)
    run_negative_xray_scenario
    ;;
  *)
    echo "unsupported SCENARIO: ${SCENARIO}" >&2
    exit 2
    ;;
esac
