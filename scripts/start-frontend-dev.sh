#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_DIR="$ROOT_DIR/logs"
PID_FILE="$LOG_DIR/frontend.pid"
LOG_FILE="$LOG_DIR/web.dev.log"
APP_DIR="$ROOT_DIR/web"

mkdir -p "$LOG_DIR"

if [[ -f "$PID_FILE" ]]; then
  EXISTING_PID="$(cat "$PID_FILE" || true)"
  if [[ -n "$EXISTING_PID" ]] && kill -0 "$EXISTING_PID" 2>/dev/null; then
    echo "Existing frontend process (PID $EXISTING_PID) detected. Stopping it first..."
    kill "$EXISTING_PID" && sleep 1
  fi
  rm -f "$PID_FILE"
fi

HOST="${HOST:-127.0.0.1}"
PORT="${PORT:-55173}"

pushd "$APP_DIR" >/dev/null

if [[ ! -d node_modules ]]; then
  echo "node_modules missing; installing dependencies via bun install..."
  bun install --frozen-lockfile
fi

echo "Starting frontend dev server on $HOST:$PORT (logging to $LOG_FILE)..."
nohup bun run dev -- --host "$HOST" --port "$PORT" >"$LOG_FILE" 2>&1 &
FRONTEND_PID=$!
echo "$FRONTEND_PID" > "$PID_FILE"

popd >/dev/null

echo "Frontend started with PID $FRONTEND_PID"
