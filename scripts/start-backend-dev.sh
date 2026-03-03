#!/usr/bin/env bash
set -euo pipefail

# Resolve repo root relative to this script
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

PORT="${PORT:-58087}"
BIND_ADDR="${BIND_ADDR:-127.0.0.1}"
DB_PATH="${DB_PATH:-tavily_proxy.db}"
STATIC_DIR="${STATIC_DIR:-web/dist}"
RUST_LOG="${RUST_LOG:-info}"

pushd "$ROOT_DIR" >/dev/null

CMD=(cargo run --bin tavily-hikari -- --bind "$BIND_ADDR" --port "$PORT" --db-path "$DB_PATH")
if [[ -d "$STATIC_DIR" ]]; then
  CMD+=(--static-dir "$STATIC_DIR")
fi
if [[ "${DEV_OPEN_ADMIN:-}" == "true" || "${DEV_OPEN_ADMIN:-}" == "1" ]]; then
  CMD+=(--dev-open-admin)
fi

echo "Starting backend in foreground on $BIND_ADDR:$PORT..."
exec env RUST_LOG="$RUST_LOG" "${CMD[@]}"
