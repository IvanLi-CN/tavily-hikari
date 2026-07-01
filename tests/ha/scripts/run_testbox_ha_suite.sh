#!/usr/bin/env bash
set -euo pipefail

REMOTE_RUN="${REMOTE_RUN:?REMOTE_RUN is required}"
COMPOSE_PROJECT="${COMPOSE_PROJECT:?COMPOSE_PROJECT is required}"
HA_RUNTIME_DIR="${HA_RUNTIME_DIR:?HA_RUNTIME_DIR is required}"

COMPOSE_FILE="${COMPOSE_FILE:-tests/ha/docker-compose.yml}"
LEGACY_COMPOSE_FILE="${LEGACY_COMPOSE_FILE:-tests/ha/docker-compose.legacy.yml}"
DUAL_ACTIVE_COMPOSE_FILE="${DUAL_ACTIVE_COMPOSE_FILE:-tests/ha/docker-compose.dual-active.yml}"
MEMORY_COMPOSE_FILE="${MEMORY_COMPOSE_FILE:-tests/ha/docker-compose.memory.yml}"
KEEP_REMOTE_RUN_ON_SUCCESS="${KEEP_REMOTE_RUN_ON_SUCCESS:-false}"
HA_INTERNAL_TOKEN="${HA_INTERNAL_TOKEN:-ha-internal-token}"
SUMMARY_PATH="${SUMMARY_PATH:-${REMOTE_RUN}/ha-suite-summary.json}"
export HA_RUNTIME_UID="${HA_RUNTIME_UID:-$(id -u)}"
export HA_RUNTIME_GID="${HA_RUNTIME_GID:-$(id -g)}"

TMP_DIR="$(mktemp -d)"
CAPS_OVERRIDE_FILE="${TMP_DIR}/caps-compat.yml"
NETWORK_OVERRIDE_FILE="${TMP_DIR}/network-override.yml"
ARTIFACT_DIR="${REMOTE_RUN}/artifacts"

mkdir -p "${ARTIFACT_DIR}" "${HA_RUNTIME_DIR}/node-a" "${HA_RUNTIME_DIR}/node-b"

docker_compose_cmd() {
  if docker compose version >/dev/null 2>&1; then
    HA_RUNTIME_DIR="${HA_RUNTIME_DIR}" docker compose "$@"
  else
    HA_RUNTIME_DIR="${HA_RUNTIME_DIR}" docker-compose "$@"
  fi
}

compose() {
  local overlay="${1:?overlay required}"
  shift
  docker_compose_cmd \
    -p "${COMPOSE_PROJECT}" \
    -f "${COMPOSE_FILE}" \
    -f "${overlay}" \
    -f "${CAPS_OVERRIDE_FILE}" \
    -f "${NETWORK_OVERRIDE_FILE}" \
    "$@"
}

generate_caps_override() {
  local services
  services="$(docker_compose_cmd -f "${COMPOSE_FILE}" -f "${LEGACY_COMPOSE_FILE}" config --services)"
  {
    echo "services:"
    while IFS= read -r service; do
      [[ -n "${service}" ]] || continue
      cat <<YAML
  ${service}:
    cap_drop:
      - ALL
    cap_add:
      - CHOWN
      - DAC_OVERRIDE
      - FSETID
      - FOWNER
      - MKNOD
      - NET_RAW
      - SETGID
      - SETUID
      - SETPCAP
      - NET_BIND_SERVICE
      - SYS_CHROOT
      - KILL
      - AUDIT_WRITE
YAML
    done <<<"${services}"
  } > "${CAPS_OVERRIDE_FILE}"
}

collect_used_subnets() {
  docker network ls -q \
    | xargs -r docker network inspect --format '{{range .IPAM.Config}}{{.Subnet}}{{"\n"}}{{end}}' 2>/dev/null \
    | sed '/^$/d' \
    | sort -u
}

pick_compose_subnet() {
  local used candidate second_octet third_octet
  used="$(collect_used_subnets || true)"
  for second_octet in 250 251 252 253 254 255; do
    for third_octet in $(seq 0 255); do
      candidate="10.${second_octet}.${third_octet}.0/24"
      if ! grep -qx "${candidate}" <<<"${used}"; then
        printf '%s\n' "${candidate}"
        return 0
      fi
    done
  done
  echo "unable to allocate an isolated docker subnet for ${COMPOSE_PROJECT}" >&2
  return 1
}

generate_network_override() {
  local subnet
  subnet="$(pick_compose_subnet)"
  cat > "${NETWORK_OVERRIDE_FILE}" <<YAML
networks:
  default:
    ipam:
      config:
        - subnet: ${subnet}
YAML
}

wait_for_health() {
  local overlay="$1"
  local service="$2"
  for _ in $(seq 1 90); do
    if compose "${overlay}" exec -T "${service}" curl -fsS http://127.0.0.1:8787/health >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  echo "timed out waiting for ${service} health (${overlay})" >&2
  return 1
}

service_ip() {
  local overlay="$1"
  local service="$2"
  docker inspect "$(compose "${overlay}" ps -q "${service}")" \
    --format '{{range.NetworkSettings.Networks}}{{.IPAddress}}{{end}}'
}

clear_runtime_dir() {
  rm -rf "${HA_RUNTIME_DIR}/node-a" "${HA_RUNTIME_DIR}/node-b"
  mkdir -p "${HA_RUNTIME_DIR}/node-a" "${HA_RUNTIME_DIR}/node-b"
}

collect_logs() {
  local overlay="$1"
  local name="$2"
  compose "${overlay}" logs --no-color > "${ARTIFACT_DIR}/${name}.log" 2>&1 || true
}

run_acceptance_stage() {
  local overlay="$1"
  local stage="$2"
  local outfile="$3"
  local node_a_ip node_b_ip ingress_ip
  node_a_ip="$(service_ip "${overlay}" node-a)"
  node_b_ip="$(service_ip "${overlay}" node-b)"
  ingress_ip="$(service_ip "${overlay}" edgeone-ingress)"
  EDGEONE_IP="$(service_ip "${overlay}" edgeone-mock)"
  UPSTREAM_IP="$(service_ip "${overlay}" upstream-mock)"
  INGRESS_URL="http://${ingress_ip}:8080" \
    EDGEONE_MOCK_URL="http://${EDGEONE_IP}:9000" \
    UPSTREAM_MOCK_URL="http://${UPSTREAM_IP}:9001" \
    NODE_A_URL="http://${node_a_ip}:8787" \
    NODE_B_URL="http://${node_b_ip}:8787" \
    HA_ACCEPTANCE_STATE_FILE="${REMOTE_RUN}/ha-acceptance-state.json" \
    python3 tests/ha/scripts/run_ha_acceptance.py "${stage}" > "${outfile}"
}

run_legacy_suite() {
  local overlay="${LEGACY_COMPOSE_FILE}"
  (
    set -euo pipefail
    clear_runtime_dir
    compose "${overlay}" build node-a node-b edgeone-mock edgeone-ingress upstream-mock
    compose "${overlay}" up -d
    wait_for_health "${overlay}" node-a
    wait_for_health "${overlay}" node-b
    run_acceptance_stage "${overlay}" legacy_pre "${ARTIFACT_DIR}/legacy_pre.json"
    run_acceptance_stage "${overlay}" legacy_failover "${ARTIFACT_DIR}/legacy_failover.json"
    run_acceptance_stage "${overlay}" legacy_recovery "${ARTIFACT_DIR}/legacy_recovery.json"
  )
}

run_dual_active_suite() {
  local overlay="${DUAL_ACTIVE_COMPOSE_FILE}"
  (
    set -euo pipefail
    clear_runtime_dir
    rm -f "${REMOTE_RUN}/ha-acceptance-state.json"
    compose "${overlay}" build node-a node-b edgeone-mock edgeone-ingress upstream-mock
    compose "${overlay}" up -d
    wait_for_health "${overlay}" node-a
    wait_for_health "${overlay}" node-b
    run_acceptance_stage "${overlay}" dual_active_serving "${ARTIFACT_DIR}/dual_active_serving.json"
    run_acceptance_stage "${overlay}" dual_active_cutover "${ARTIFACT_DIR}/dual_active_cutover.json"
  )
}

run_memory_suite() {
  local outfile="${ARTIFACT_DIR}/memory_contract.json"
  (
    set -euo pipefail
    COMPOSE_FILE="${COMPOSE_FILE}" \
    DUAL_ACTIVE_COMPOSE_FILE="${DUAL_ACTIVE_COMPOSE_FILE}" \
    MEMORY_COMPOSE_FILE="${MEMORY_COMPOSE_FILE}" \
    COMPOSE_PROJECT="${COMPOSE_PROJECT}" \
    HA_RUNTIME_DIR="${HA_RUNTIME_DIR}" \
    HA_INTERNAL_TOKEN="${HA_INTERNAL_TOKEN}" \
    bash tests/ha/scripts/run_testbox_ha_memory_contract.sh > "${outfile}"
  )
}

generate_caps_override
generate_network_override

trap 'rm -rf "${TMP_DIR}"' EXIT

legacy_status=0
dual_status=0
memory_status=0

set +e
run_legacy_suite
legacy_status="$?"
collect_logs "${LEGACY_COMPOSE_FILE}" legacy
compose "${LEGACY_COMPOSE_FILE}" down -v --remove-orphans >/dev/null 2>&1 || true
run_dual_active_suite
dual_status="$?"
collect_logs "${DUAL_ACTIVE_COMPOSE_FILE}" dual_active
compose "${DUAL_ACTIVE_COMPOSE_FILE}" down -v --remove-orphans >/dev/null 2>&1 || true
run_memory_suite
memory_status="$?"
set -e

python3 - <<'PY' \
  "${SUMMARY_PATH}" \
  "${REMOTE_RUN}" \
  "${legacy_status}" \
  "${dual_status}" \
  "${memory_status}" \
  "${ARTIFACT_DIR}"
import json
import pathlib
import sys

summary_path = pathlib.Path(sys.argv[1])
remote_run = sys.argv[2]
legacy_status = int(sys.argv[3])
dual_status = int(sys.argv[4])
memory_status = int(sys.argv[5])
artifact_dir = pathlib.Path(sys.argv[6])

def load_json(name):
    path = artifact_dir / name
    if not path.exists():
        return None
    try:
        return json.loads(path.read_text())
    except json.JSONDecodeError:
        return {"raw": path.read_text()}

payload = {
    "remoteRun": remote_run,
    "composeProject": pathlib.Path(remote_run).name,
    "legacy": {
        "status": legacy_status,
        "pre": load_json("legacy_pre.json"),
        "failover": load_json("legacy_failover.json"),
        "recovery": load_json("legacy_recovery.json"),
    },
    "dualActive": {
        "status": dual_status,
        "serving": load_json("dual_active_serving.json"),
        "cutover": load_json("dual_active_cutover.json"),
    },
    "memory": {
        "status": memory_status,
        "contract": load_json("memory_contract.json"),
    },
}
summary_path.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n")
print(json.dumps(payload, ensure_ascii=False))
PY

if [[ "${legacy_status}" -ne 0 || "${dual_status}" -ne 0 || "${memory_status}" -ne 0 ]]; then
  exit 1
fi

if [[ "${KEEP_REMOTE_RUN_ON_SUCCESS}" != "true" ]]; then
  echo "suite succeeded; caller may remove ${REMOTE_RUN}" >&2
fi
