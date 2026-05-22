#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_DIR="$ROOT_DIR/web"

HOST="${HOST:-127.0.0.1}"
PORT="${PORT:-55174}"

pushd "$APP_DIR" >/dev/null

if [[ ! -d node_modules ]]; then
  echo "node_modules missing; installing dependencies via bun install..."
  bun install --frozen-lockfile
fi

echo "Starting pure web demo mode in foreground on $HOST:$PORT..."
echo "Demo mode uses browser-local API and SSE mocks; no backend is required."
exec env VITE_DEMO_MODE=true bun run --bun dev -- --host "$HOST" --port "$PORT"
