#!/usr/bin/env bash
set -Eeuo pipefail

LOCAL_SMOKE_IMAGE="${LOCAL_SMOKE_IMAGE:-}"
if [[ -z "${LOCAL_SMOKE_IMAGE}" ]]; then
  echo "LOCAL_SMOKE_IMAGE is required" >&2
  exit 2
fi

SMOKE_MOCK_BIN="${SMOKE_MOCK_BIN:-./target/debug/mock_tavily}"
if [[ ! -x "${SMOKE_MOCK_BIN}" ]]; then
  echo "mock_tavily binary is missing or not executable: ${SMOKE_MOCK_BIN}" >&2
  exit 2
fi

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
exclude = {int(part) for part in sys.argv[2].split(',') if part}
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
DATA_DIR="${SMOKE_DATA_DIR:-${TEMP_ROOT}/tavily-hikari-release-smoke-data-${MOCK_PORT}-${PROXY_PORT}}"
DB_PATH="${DATA_DIR}/tavily_proxy.db"
MOCK_LOG="${SMOKE_MOCK_LOG:-${TEMP_ROOT}/mock-tavily-${MOCK_PORT}.log}"
CONTAINER_NAME="${SMOKE_CONTAINER_NAME:-tavily-hikari-release-smoke-${MOCK_PORT}-${PROXY_PORT}}"
MOCK_BASE_URL="http://${MOCK_HOST}:${MOCK_PORT}"
PROXY_BASE_URL="http://${PROXY_HOST}:${PROXY_PORT}"
# Keep mock_tavily bound to 0.0.0.0 by default: the release container reaches it via
# host.docker.internal:host-gateway, and a host-loopback-only bind would not be reachable from the bridge network.
MOCK_BIND_ADDR="${MOCK_BIND_HOST}:${MOCK_PORT}"

MOCK_PID=""
DIAGNOSTICS_EMITTED=0

print_section() {
  local title="$1"
  echo "::group::${title}"
}

end_section() {
  echo "::endgroup::"
}

container_exists() {
  if ! command -v docker >/dev/null 2>&1; then
    return 1
  fi
  docker container inspect "${CONTAINER_NAME}" >/dev/null 2>&1
}

container_status() {
  if ! command -v docker >/dev/null 2>&1; then
    return 0
  fi
  docker container inspect --format '{{.State.Status}}' "${CONTAINER_NAME}" 2>/dev/null || true
}

dump_mock_log() {
  if [[ -f "${MOCK_LOG}" ]]; then
    print_section "mock_tavily log"
    cat "${MOCK_LOG}" || true
    end_section
  else
    echo "mock_tavily log file missing: ${MOCK_LOG}" >&2
  fi
}

dump_port_state() {
  print_section "release smoke port state"
  echo "mock_port=${MOCK_PORT} proxy_port=${PROXY_PORT}"
  if command -v ss >/dev/null 2>&1; then
    ss -lntp | grep -E ":(${MOCK_PORT}|${PROXY_PORT})\\b" || true
  elif command -v lsof >/dev/null 2>&1; then
    lsof -nP -iTCP:"${MOCK_PORT}" -iTCP:"${PROXY_PORT}" || true
  else
    echo "no ss/lsof available for port diagnostics" >&2
  fi
  end_section
}

dump_docker_state() {
  if ! command -v docker >/dev/null 2>&1; then
    return 0
  fi
  print_section "release smoke docker state"
  docker ps -a || true
  if container_exists; then
    echo "container_status=$(container_status)"
    docker logs "${CONTAINER_NAME}" || true
  else
    echo "container not created: ${CONTAINER_NAME}"
  fi
  end_section
}

dump_data_dir_state() {
  print_section "release smoke data dir state"
  if [[ -d "${DATA_DIR}" ]]; then
    ls -la "${DATA_DIR}" || true
    find "${DATA_DIR}" -maxdepth 2 -mindepth 1 -print | sort || true
  else
    echo "data dir missing: ${DATA_DIR}" >&2
  fi
  if [[ -f "${DB_PATH}" ]]; then
    echo "db_path=${DB_PATH}"
    ls -la "${DB_PATH}" || true
  fi
  end_section
}

dump_diagnostics() {
  if [[ "${DIAGNOSTICS_EMITTED}" == "1" ]]; then
    return 0
  fi
  DIAGNOSTICS_EMITTED=1
  dump_mock_log || true
  dump_port_state || true
  dump_docker_state || true
  dump_data_dir_state || true
}

cleanup() {
  set +e
  if container_exists; then
    docker rm -f "${CONTAINER_NAME}" >/dev/null 2>&1 || true
  fi
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
  echo "release smoke failed at line ${line_no}: ${command} (exit=${exit_code})" >&2
  dump_diagnostics
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

wait_for_proxy_ready() {
  local url="${PROXY_BASE_URL}/health"
  local attempt
  for attempt in {1..60}; do
    if curl -fsS --max-time 2 "${url}" >/dev/null; then
      return 0
    fi
    local status
    status="$(container_status)"
    if [[ -n "${status}" && "${status}" != "running" ]]; then
      echo "release smoke proxy container is not running (status=${status})" >&2
      return 1
    fi
    sleep 1
  done
  echo "timed out waiting for proxy health: ${url}" >&2
  return 1
}

mkdir -p "${DATA_DIR}"

echo "release smoke config: mock=${MOCK_BIND_ADDR} proxy=${PROXY_BASE_URL} container=${CONTAINER_NAME} image=${LOCAL_SMOKE_IMAGE}"

"${SMOKE_MOCK_BIN}" --bind "${MOCK_BIND_ADDR}" >"${MOCK_LOG}" 2>&1 &
MOCK_PID=$!
wait_for_mock_ready

curl -fsS \
  -X POST \
  "${MOCK_BASE_URL}/admin/keys" \
  -H 'content-type: application/json' \
  -d '{"secret":"tvly-test-key","limit":1000,"remaining":1000}' \
  >/dev/null

docker run -d --rm \
  --name "${CONTAINER_NAME}" \
  --add-host host.docker.internal:host-gateway \
  -p "${PROXY_HOST}:${PROXY_PORT}:8787" \
  -v "${DATA_DIR}:/srv/app/data" \
  -e TAVILY_API_KEYS=tvly-test-key \
  -e TAVILY_UPSTREAM="http://host.docker.internal:${MOCK_PORT}/mcp" \
  -e TAVILY_USAGE_BASE="http://host.docker.internal:${MOCK_PORT}" \
  -e DEV_OPEN_ADMIN=true \
  -e PROXY_DB_PATH=/srv/app/data/tavily_proxy.db \
  "${LOCAL_SMOKE_IMAGE}" \
  >/dev/null

wait_for_proxy_ready

TOKEN_PAYLOAD="$({
  curl -fsS \
    -X POST \
    "${PROXY_BASE_URL}/api/tokens" \
    -H 'content-type: application/json' \
    -d '{}'
})"
export TOKEN_PAYLOAD
TOKEN="$(python3 - <<'PY'
import json, os
payload = json.loads(os.environ["TOKEN_PAYLOAD"])
print(payload["token"])
PY
)"
TOKEN_ID="$(python3 - <<'PY'
import json
import os
payload = json.loads(os.environ["TOKEN_PAYLOAD"])
full_token = payload["token"]
token_id = full_token.removeprefix("th-").split("-", 1)[0]
print(token_id)
PY
)"

SEARCH_RESPONSE="$({
  curl -fsS \
    -X POST \
    "${PROXY_BASE_URL}/mcp" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H 'Accept: application/json, text/event-stream' \
    -H 'content-type: application/json' \
    -d '{"jsonrpc":"2.0","id":"release-smoke-search","method":"tools/call","params":{"name":"tavily_search","arguments":{"query":"release smoke gate","search_depth":"advanced"}}}'
})"
export SEARCH_RESPONSE
python3 - <<'PY'
import json, os
payload = json.loads(os.environ["SEARCH_RESPONSE"])
assert payload.get("result"), payload
PY

curl -fsS \
  -X POST \
  "${PROXY_BASE_URL}/mcp" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H 'Accept: application/json, text/event-stream' \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  >/dev/null

curl -fsS \
  -X POST \
  "${MOCK_BASE_URL}/admin/force-response" \
  -H 'content-type: application/json' \
  -d '{"http_status":406,"body":{"error":"client must accept both application/json and text/event-stream"},"once":true}' \
  >/dev/null

CLIENT_ERROR_HTTP="$({
  curl -sS \
    -o /dev/null \
    -w '%{http_code}' \
    -X POST \
    "${PROXY_BASE_URL}/mcp" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H 'Accept: application/json, text/event-stream' \
    -H 'content-type: application/json' \
    -d '{"jsonrpc":"2.0","id":"release-smoke-406","method":"tools/call","params":{"name":"tavily_search","arguments":{"query":"release smoke client error"}}}'
})"
if [[ "${CLIENT_ERROR_HTTP}" != "406" ]]; then
  echo "expected 406 MCP client error, got ${CLIENT_ERROR_HTTP}" >&2
  exit 1
fi

curl -fsS \
  -X POST \
  "${MOCK_BASE_URL}/admin/force-response" \
  -H 'content-type: application/json' \
  -d '{"http_status":429,"body":{"error":"release smoke upstream rate limit"},"once":true}' \
  >/dev/null

UPSTREAM_ERROR_HTTP="$({
  curl -sS \
    -o /dev/null \
    -w '%{http_code}' \
    -X POST \
    "${PROXY_BASE_URL}/mcp" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H 'Accept: application/json, text/event-stream' \
    -H 'content-type: application/json' \
    -d '{"jsonrpc":"2.0","id":"release-smoke-429","method":"tools/call","params":{"name":"tavily_search","arguments":{"query":"release smoke upstream error"}}}'
})"
if [[ "${UPSTREAM_ERROR_HTTP}" != "429" ]]; then
  echo "expected 429 MCP upstream failure, got ${UPSTREAM_ERROR_HTTP}" >&2
  exit 1
fi

python3 - "${DB_PATH}" "${TOKEN_ID}" <<'PY'
import json
import pathlib
import sqlite3
import sys

db_path = pathlib.Path(sys.argv[1])
token_id = sys.argv[2]
conn = sqlite3.connect(db_path)
request_rows = conn.execute(
    """
    SELECT request_body, result_status, failure_kind, request_kind_key
    FROM request_logs
    WHERE auth_token_id = ?
    ORDER BY id DESC
    """,
    (token_id,),
).fetchall()
if not request_rows:
    raise SystemExit("missing request_logs row for smoke token")

success_search_rows = []
cleaned_body_rows = 0
for request_body_raw, result_status, failure_kind, request_kind_key in request_rows:
    if request_body_raw is None:
        cleaned_body_rows += 1
        continue
    request_body = json.loads(request_body_raw.decode())
    request_id = request_body.get("id")
    if request_id == "release-smoke-search":
        success_search_rows.append((request_body, result_status, failure_kind))

if cleaned_body_rows < 1:
    raise SystemExit(
        "expected at least one request_logs row with a cleaned or omitted body under release smoke retention policy"
    )

if len(success_search_rows) != 1:
    raise SystemExit(
        f"expected exactly one successful MCP search request_logs row for smoke token, got {len(success_search_rows)}"
    )

request_body, result_status, failure_kind = success_search_rows[0]
if request_body["params"]["arguments"].get("include_usage") is not None:
    raise SystemExit(f"include_usage must not be forwarded for MCP smoke: {request_body}")
if result_status != "success" or failure_kind is not None:
    raise SystemExit(
        f"unexpected request_logs outcome for MCP smoke search: result={result_status} failure={failure_kind}"
    )

token_log_row = conn.execute(
    "SELECT business_credits FROM auth_token_logs WHERE token_id = ? AND request_kind_key = 'mcp:search' AND result_status = 'success' ORDER BY id DESC LIMIT 1",
    (token_id,),
).fetchone()
if token_log_row is None or token_log_row[0] is None or token_log_row[0] <= 0:
    raise SystemExit(f"missing charged credits for smoke token: {token_log_row}")

month_row = conn.execute(
    "SELECT COALESCE(month_count, 0) FROM auth_token_quota WHERE token_id = ? LIMIT 1",
    (token_id,),
).fetchone()
if month_row is None or month_row[0] < token_log_row[0]:
    raise SystemExit(
        f"token monthly quota did not increase with billed credits: month_row={month_row} charged={token_log_row}"
    )
PY

ADMIN_LOGS_JSON="$({
  curl -fsS "${PROXY_BASE_URL}/api/logs?page=1&per_page=20"
})"
TOKEN_LOGS_NEUTRAL_JSON="$({
  curl -fsS "${PROXY_BASE_URL}/api/tokens/${TOKEN_ID}/logs/page?page=1&per_page=20&since=0&operational_class=neutral"
})"
export ADMIN_LOGS_JSON TOKEN_LOGS_NEUTRAL_JSON
python3 - <<'PY'
import json
import os

admin_logs = json.loads(os.environ["ADMIN_LOGS_JSON"])
neutral_token_logs = json.loads(os.environ["TOKEN_LOGS_NEUTRAL_JSON"])

def find_log(items, predicate, label):
    for item in items:
        if predicate(item):
            return item
    raise SystemExit(f"missing {label} in admin logs: {items}")

items = admin_logs.get("items") or []
neutral_log = find_log(
    items,
    lambda item: item.get("request_kind_key") == "mcp:notifications/initialized",
    "neutral MCP notification",
)
if neutral_log.get("operationalClass") != "neutral":
    raise SystemExit(f"expected neutral operational class, got {neutral_log}")
if neutral_log.get("requestKindProtocolGroup") != "mcp":
    raise SystemExit(f"expected MCP protocol group, got {neutral_log}")
if neutral_log.get("requestKindBillingGroup") != "non_billable":
    raise SystemExit(f"expected non-billable MCP notification, got {neutral_log}")

client_error_log = find_log(
    items,
    lambda item: item.get("failure_kind") == "mcp_accept_406",
    "client_error MCP accept failure",
)
if client_error_log.get("operationalClass") != "client_error":
    raise SystemExit(f"expected client_error operational class, got {client_error_log}")

upstream_error_log = find_log(
    items,
    lambda item: item.get("failure_kind") == "upstream_rate_limited_429",
    "upstream_error MCP rate limit",
)
if upstream_error_log.get("operationalClass") != "upstream_error":
    raise SystemExit(f"expected upstream_error operational class, got {upstream_error_log}")

neutral_items = neutral_token_logs.get("items") or []
if neutral_token_logs.get("total") != 1 or len(neutral_items) != 1:
    raise SystemExit(f"expected exactly one neutral token log row, got {neutral_token_logs}")
neutral_token_log = neutral_items[0]
if neutral_token_log.get("request_kind_key") != "mcp:notifications/initialized":
    raise SystemExit(f"unexpected neutral token log row: {neutral_token_log}")
if neutral_token_log.get("operationalClass") != "neutral":
    raise SystemExit(f"expected neutral token log operational class, got {neutral_token_log}")
PY

echo "release smoke passed: mock_port=${MOCK_PORT} proxy_port=${PROXY_PORT} token_id=${TOKEN_ID}"
