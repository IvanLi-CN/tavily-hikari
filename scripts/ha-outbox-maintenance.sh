#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

DB_PATH="${DB_PATH:-tavily_proxy.db}"
RUN_UNTIL_COMPLETE="${RUN_UNTIL_COMPLETE:-true}"
JSON="${JSON:-true}"
COMPACT_AFTER="${COMPACT_AFTER:-false}"
FORCE_COMPACTION="${FORCE_COMPACTION:-false}"
REPAIR_TRIGGERS="${REPAIR_TRIGGERS:-true}"
HA_MODE="${HA_MODE:-active_standby}"
BATCH_SIZE="${BATCH_SIZE:-20000}"
MAX_BATCHES="${MAX_BATCHES:-8}"
MAX_RUNTIME_SECS="${MAX_RUNTIME_SECS:-20}"
INTER_BATCH_SLEEP_MS="${INTER_BATCH_SLEEP_MS:-0}"

resolve_runner() {
  local bin_name="$1"
  if [[ -n "${TAVILY_HIKARI_BIN_DIR:-}" && -x "${TAVILY_HIKARI_BIN_DIR}/${bin_name}" ]]; then
    printf '%s\n' "${TAVILY_HIKARI_BIN_DIR}/${bin_name}"
    return 0
  fi
  if command -v "$bin_name" >/dev/null 2>&1; then
    command -v "$bin_name"
    return 0
  fi
  printf 'cargo-run:%s\n' "$bin_name"
}

build_command() {
  local bin_name="$1"
  local resolved
  resolved="$(resolve_runner "$bin_name")"
  if [[ "$resolved" == cargo-run:* ]]; then
    printf 'cargo run --bin %s --' "$bin_name"
  else
    printf '%s' "$resolved"
  fi
}

pushd "$ROOT_DIR" >/dev/null

cleanup_runner="$(build_command ha_outbox_cleanup_once)"
IFS=' ' read -r -a cleanup_cmd <<<"$cleanup_runner"
cleanup_cmd+=(
  --db-path "$DB_PATH"
  --batch-size "$BATCH_SIZE"
  --max-batches "$MAX_BATCHES"
  --max-runtime-secs "$MAX_RUNTIME_SECS"
  --inter-batch-sleep-ms "$INTER_BATCH_SLEEP_MS"
  --ha-mode "$HA_MODE"
)

if [[ "$REPAIR_TRIGGERS" == "true" || "$REPAIR_TRIGGERS" == "1" ]]; then
  cleanup_cmd+=(--repair-triggers)
fi

if [[ "$RUN_UNTIL_COMPLETE" == "true" || "$RUN_UNTIL_COMPLETE" == "1" ]]; then
  cleanup_cmd+=(--run-until-complete)
fi

if [[ "$JSON" == "true" || "$JSON" == "1" ]]; then
  cleanup_cmd+=(--json)
fi

echo "Running HA outbox cleanup against $DB_PATH ..."
"${cleanup_cmd[@]}"

if [[ "$COMPACT_AFTER" == "true" || "$COMPACT_AFTER" == "1" ]]; then
  compaction_runner="$(build_command db_compaction_once)"
  IFS=' ' read -r -a compaction_cmd <<<"$compaction_runner"
  compaction_cmd+=(--db-path "$DB_PATH")
  if [[ "$FORCE_COMPACTION" == "true" || "$FORCE_COMPACTION" == "1" ]]; then
    compaction_cmd+=(--force)
  fi
  if [[ "$JSON" == "true" || "$JSON" == "1" ]]; then
    compaction_cmd+=(--json)
  fi
  echo "Running SQLite compaction against $DB_PATH ..."
  "${compaction_cmd[@]}"
fi

popd >/dev/null
