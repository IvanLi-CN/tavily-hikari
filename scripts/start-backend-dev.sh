#!/usr/bin/env bash
set -euo pipefail

# Resolve repo root relative to this script
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_DIR="$ROOT_DIR/logs"
PID_FILE="$LOG_DIR/backend.pid"
LOG_FILE="$LOG_DIR/backend.dev.log"

mkdir -p "$LOG_DIR"

if [[ -f "$PID_FILE" ]]; then
  EXISTING_PID="$(cat "$PID_FILE" || true)"
  if [[ -n "$EXISTING_PID" ]] && kill -0 "$EXISTING_PID" 2>/dev/null; then
    echo "Existing backend process (PID $EXISTING_PID) detected. Stopping it first..."
    kill "$EXISTING_PID" && sleep 1
  fi
  rm -f "$PID_FILE"
fi

PORT="${PORT:-58087}"
BIND_ADDR="${BIND_ADDR:-127.0.0.1}"
DB_PATH="${DB_PATH:-tavily_proxy.db}"
STATIC_DIR="${STATIC_DIR:-web/dist}"
RUST_LOG="${RUST_LOG:-info}"

pushd "$ROOT_DIR" >/dev/null

echo "Starting backend on $BIND_ADDR:$PORT (logging to $LOG_FILE)..."
CMD=(cargo run --bin tavily-hikari -- --bind "$BIND_ADDR" --port "$PORT" --db-path "$DB_PATH")
if [[ -d "$STATIC_DIR" ]]; then
  CMD+=(--static-dir "$STATIC_DIR")
fi
if [[ "${DEV_OPEN_ADMIN:-}" == "true" || "${DEV_OPEN_ADMIN:-}" == "1" ]]; then
  CMD+=(--dev-open-admin)
fi
nohup env RUST_LOG="$RUST_LOG" "${CMD[@]}" >"$LOG_FILE" 2>&1 &
BACKEND_PID=$!
echo "$BACKEND_PID" > "$PID_FILE"

popd >/dev/null

echo "Backend started with PID $BACKEND_PID"
