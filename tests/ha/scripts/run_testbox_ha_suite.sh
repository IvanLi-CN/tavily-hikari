#!/usr/bin/env bash
set -euo pipefail

REMOTE_RUN="${REMOTE_RUN:?REMOTE_RUN is required}"
COMPOSE_PROJECT="${COMPOSE_PROJECT:?COMPOSE_PROJECT is required}"
HA_RUNTIME_DIR="${HA_RUNTIME_DIR:?HA_RUNTIME_DIR is required}"

COMPOSE_FILE="${COMPOSE_FILE:-tests/ha/docker-compose.yml}"
SUMMARY_PATH="${SUMMARY_PATH:-${REMOTE_RUN}/ha-suite-summary.json}"
ARTIFACT_DIR="${REMOTE_RUN}/artifacts"

export HA_RUNTIME_UID="${HA_RUNTIME_UID:-$(id -u)}"
export HA_RUNTIME_GID="${HA_RUNTIME_GID:-$(id -g)}"

TMP_DIR="$(mktemp -d)"
CAPS_OVERRIDE_FILE="${TMP_DIR}/caps-compat.yml"
NETWORK_OVERRIDE_FILE="${TMP_DIR}/network-override.yml"

mkdir -p "${ARTIFACT_DIR}" "${HA_RUNTIME_DIR}/node-a"

docker_compose_cmd() {
  if docker compose version >/dev/null 2>&1; then
    HA_RUNTIME_DIR="${HA_RUNTIME_DIR}" docker compose "$@"
  else
    HA_RUNTIME_DIR="${HA_RUNTIME_DIR}" docker-compose "$@"
  fi
}

compose() {
  docker_compose_cmd \
    -p "${COMPOSE_PROJECT}" \
    -f "${COMPOSE_FILE}" \
    -f "${CAPS_OVERRIDE_FILE}" \
    -f "${NETWORK_OVERRIDE_FILE}" \
    "$@"
}

cleanup() {
  compose logs --no-color > "${ARTIFACT_DIR}/single_node.log" 2>&1 || true
  compose down -v --remove-orphans >/dev/null 2>&1 || true
  rm -rf "${TMP_DIR}"
}
trap cleanup EXIT

generate_caps_override() {
  local services
  services="$(docker_compose_cmd -f "${COMPOSE_FILE}" config --services)"
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
  cat <<YAML > "${NETWORK_OVERRIDE_FILE}"
networks:
  default:
    ipam:
      config:
        - subnet: ${subnet}
YAML
}

wait_for_health() {
  for _ in $(seq 1 90); do
    if compose exec -T node-a curl -fsS http://127.0.0.1:8787/health >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  echo "timed out waiting for node-a health" >&2
  return 1
}

service_ip() {
  docker inspect "$(compose ps -q node-a)" \
    --format '{{range.NetworkSettings.Networks}}{{.IPAddress}}{{end}}'
}

generate_caps_override
generate_network_override

compose build node-a upstream-mock
compose up -d node-a upstream-mock
wait_for_health

NODE_IP="$(service_ip)"
NODE_URL="http://${NODE_IP}:8787" \
  python3 tests/ha/scripts/run_single_node_acceptance.py \
  > "${ARTIFACT_DIR}/single_node.json"
compose logs --no-color > "${ARTIFACT_DIR}/single_node.log" 2>&1 || true

python3 - <<'PY' "${SUMMARY_PATH}" "${ARTIFACT_DIR}/single_node.json"
import json
import pathlib
import sys

summary_path = pathlib.Path(sys.argv[1])
result_path = pathlib.Path(sys.argv[2])
summary_path.write_text(
    json.dumps({"singleNode": json.loads(result_path.read_text())}, indent=2) + "\n"
)
print(summary_path.read_text(), end="")
PY
