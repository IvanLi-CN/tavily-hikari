#!/usr/bin/env bash
set -euo pipefail

COMPOSE_FILE="${COMPOSE_FILE:-tests/ha/docker-compose.yml}"
MEMORY_COMPOSE_FILE="${MEMORY_COMPOSE_FILE:-tests/ha/docker-compose.memory.yml}"
COMPOSE_PROJECT="${COMPOSE_PROJECT:?COMPOSE_PROJECT is required}"

USER_ROWS="${HA_MEMORY_USER_ROWS:-2000}"
TOKEN_ROWS="${HA_MEMORY_TOKEN_ROWS:-2000}"
RUNTIME_ROWS="${HA_MEMORY_RUNTIME_ROWS:-3000}"
BILLING_ROWS="${HA_MEMORY_BILLING_ROWS:-35000}"
RUNTIME_TEXT_BYTES="${HA_MEMORY_RUNTIME_TEXT_BYTES:-1024}"
BILLING_ERROR_BYTES="${HA_MEMORY_BILLING_ERROR_BYTES:-4096}"
MEMORY_LIMIT_BYTES="${HA_MEMORY_LIMIT_BYTES:-268435456}"
BILLING_EXPORT_REPETITIONS="${HA_MEMORY_BILLING_EXPORT_REPETITIONS:-5}"
HA_INTERNAL_TOKEN="${HA_INTERNAL_TOKEN:-ha-internal-token}"

TMP_DIR="$(mktemp -d)"
CAPS_OVERRIDE_FILE="${TMP_DIR}/caps-compat.yml"
NETWORK_OVERRIDE_FILE="${TMP_DIR}/network-override.yml"

compose() {
  docker compose \
    -p "${COMPOSE_PROJECT}" \
    -f "${COMPOSE_FILE}" \
    -f "${MEMORY_COMPOSE_FILE}" \
    -f "${CAPS_OVERRIDE_FILE}" \
    -f "${NETWORK_OVERRIDE_FILE}" \
    "$@"
}

cleanup() {
  compose down -v --remove-orphans >/dev/null 2>&1 || true
  rm -rf "${TMP_DIR}"
}
trap cleanup EXIT

generate_caps_override() {
  local services
  services="$(docker compose -f "${COMPOSE_FILE}" -f "${MEMORY_COMPOSE_FILE}" config --services)"
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

generate_caps_override
generate_network_override

wait_for_health() {
  local service="$1"
  for _ in $(seq 1 90); do
    if compose exec -T "${service}" curl -fsS http://127.0.0.1:8787/health >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  echo "timed out waiting for ${service} health" >&2
  return 1
}

sample_memory() {
  local cid="$1"
  local out="$2"
  while docker inspect "${cid}" >/dev/null 2>&1; do
    local now
    now="$(date +%s)"
    local value=""
    value="$(docker exec "${cid}" sh -lc 'cat /sys/fs/cgroup/memory.current 2>/dev/null || cat /sys/fs/cgroup/memory/memory.usage_in_bytes 2>/dev/null' 2>/dev/null | tr -d '\r' || true)"
    if [[ "${value}" =~ ^[0-9]+$ ]]; then
      printf '%s %s\n' "${now}" "${value}" >> "${out}"
    fi
    sleep 1
  done
}

compose build node-a node-b
compose up -d edgeone-mock upstream-mock ha-test-runner node-a
wait_for_health node-a
compose stop node-a

compose cp tests/ha/scripts/seed_large_ha_fixture.py ha-test-runner:/tmp/seed_large_ha_fixture.py
compose cp tests/ha/scripts/run_ha_memory_contract.py ha-test-runner:/tmp/run_ha_memory_contract.py

compose exec -T ha-test-runner python /tmp/seed_large_ha_fixture.py \
  --db /volumes/node-a/node-a.db \
  --user-rows "${USER_ROWS}" \
  --token-rows "${TOKEN_ROWS}" \
  --runtime-rows "${RUNTIME_ROWS}" \
  --billing-rows "${BILLING_ROWS}" \
  --runtime-text-bytes "${RUNTIME_TEXT_BYTES}" \
  --billing-error-bytes "${BILLING_ERROR_BYTES}"

compose up -d node-a node-b
wait_for_health node-a
wait_for_health node-b

NODE_A_CID="$(compose ps -q node-a)"
NODE_B_CID="$(compose ps -q node-b)"
sample_memory "${NODE_A_CID}" "${TMP_DIR}/node-a.mem" &
NODE_A_SAMPLER_PID="$!"
sample_memory "${NODE_B_CID}" "${TMP_DIR}/node-b.mem" &
NODE_B_SAMPLER_PID="$!"

set +e
compose exec -T ha-test-runner python /tmp/run_ha_memory_contract.py \
  --expected-users "${USER_ROWS}" \
  --expected-tokens "${TOKEN_ROWS}" \
  --expected-sessions "${RUNTIME_ROWS}" \
  --expected-billing "${BILLING_ROWS}" \
  --billing-export-repetitions "${BILLING_EXPORT_REPETITIONS}" \
  --ha-internal-token "${HA_INTERNAL_TOKEN}" \
  > "${TMP_DIR}/contract-result.json"
CONTRACT_STATUS="$?"
set -e

kill "${NODE_A_SAMPLER_PID}" >/dev/null 2>&1 || true
kill "${NODE_B_SAMPLER_PID}" >/dev/null 2>&1 || true
wait "${NODE_A_SAMPLER_PID}" >/dev/null 2>&1 || true
wait "${NODE_B_SAMPLER_PID}" >/dev/null 2>&1 || true

node_a_peak="$(awk 'max < $2 { max = $2 } END { print max + 0 }' "${TMP_DIR}/node-a.mem")"
node_b_peak="$(awk 'max < $2 { max = $2 } END { print max + 0 }' "${TMP_DIR}/node-b.mem")"
node_a_oom="$(docker inspect "${NODE_A_CID}" --format '{{.State.OOMKilled}}')"
node_b_oom="$(docker inspect "${NODE_B_CID}" --format '{{.State.OOMKilled}}')"

python - <<'PY' \
  "${CONTRACT_STATUS}" \
  "${MEMORY_LIMIT_BYTES}" \
  "${node_a_peak}" \
  "${node_b_peak}" \
  "${node_a_oom}" \
  "${node_b_oom}" \
  "${TMP_DIR}/contract-result.json"
import json
import pathlib
import sys

contract_status = int(sys.argv[1])
memory_limit_bytes = int(sys.argv[2])
node_a_peak = int(sys.argv[3])
node_b_peak = int(sys.argv[4])
node_a_oom = sys.argv[5].strip().lower() == "true"
node_b_oom = sys.argv[6].strip().lower() == "true"
result_path = pathlib.Path(sys.argv[7])

payload = {
    "contractStatus": contract_status,
    "memoryLimitBytes": memory_limit_bytes,
    "nodeAPeakMemoryCurrent": node_a_peak,
    "nodeBPeakMemoryCurrent": node_b_peak,
    "nodeAOomKilled": node_a_oom,
    "nodeBOomKilled": node_b_oom,
}
if result_path.exists():
    payload["contractResult"] = json.loads(result_path.read_text())
else:
    payload["contractResultMissing"] = str(result_path)

print(json.dumps(payload, ensure_ascii=False))

if contract_status != 0:
    raise SystemExit(contract_status)
if node_a_oom or node_b_oom:
    raise SystemExit("OOM detected during HA memory contract run")
if node_a_peak > memory_limit_bytes or node_b_peak > memory_limit_bytes:
    raise SystemExit("sampled memory.current peak exceeded configured limit")
PY
