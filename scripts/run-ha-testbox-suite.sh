#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TESTBOX="${TESTBOX:-codex-testbox}"
KEEP_REMOTE_RUN_ON_SUCCESS="${KEEP_REMOTE_RUN_ON_SUCCESS:-false}"

if REPO_ROOT="$(git -C "$ROOT_DIR" rev-parse --show-toplevel 2>/dev/null)"; then
  :
else
  REPO_ROOT="$ROOT_DIR"
fi
REPO_ROOT="$(python3 - "$REPO_ROOT" <<'PY'
import os
import sys
print(os.path.realpath(sys.argv[1]))
PY
)"

REPO_NAME="$(basename "$REPO_ROOT")"
PATH_HASH8="$(python3 - "$REPO_ROOT" <<'PY'
import hashlib
import os
import sys
path = os.path.realpath(sys.argv[1]).encode()
print(hashlib.sha256(path).hexdigest()[:8])
PY
)"
GIT_SHA="$(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null || echo nogit)"
RUN_ID="${RUN_ID:-$(date -u +%Y%m%d_%H%M%S)_${GIT_SHA}_ha_suite}"
WORKSPACE_SLUG="${REPO_NAME}__${PATH_HASH8}"
REMOTE_BASE="/srv/codex/workspaces/$USER"
REMOTE_WORKSPACE="$REMOTE_BASE/$WORKSPACE_SLUG"
REMOTE_RUN="$REMOTE_WORKSPACE/runs/$RUN_ID"
REMOTE_REPO_DIR="$REMOTE_RUN/repo"
REMOTE_RUNTIME_DIR="$REMOTE_RUN/runtime"
REMOTE_SUMMARY="$REMOTE_RUN/ha-suite-summary.json"

COMPOSE_PROJECT_RAW="codex_${WORKSPACE_SLUG}_${RUN_ID}"
COMPOSE_PROJECT="$(python3 - "$COMPOSE_PROJECT_RAW" <<'PY'
import re
import sys
s = sys.argv[1].lower()
s = re.sub(r'[^a-z0-9_-]+', '_', s).strip('_')
print(s[:63] if len(s) > 63 else s)
PY
)"

CREATED_UTC="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
ssh -o BatchMode=yes "$TESTBOX" "mkdir -p '$REMOTE_REPO_DIR' '$REMOTE_RUNTIME_DIR' '$REMOTE_WORKSPACE' && cat > '$REMOTE_WORKSPACE/workspace.txt'" <<TXT
local_repo_root=$REPO_ROOT
created_utc=$CREATED_UTC
remote_run=$REMOTE_RUN
TXT

rsync -az --delete \
  --exclude '.git/' \
  --exclude '.tmp/' \
  --exclude 'node_modules/' \
  --exclude 'target/' \
  --exclude 'dist/' \
  --exclude 'build/' \
  --exclude '.next/' \
  --exclude '.venv/' \
  --exclude '*.db' \
  --exclude '*.db-*' \
  "$REPO_ROOT/" "$TESTBOX:$REMOTE_REPO_DIR/"

set +e
ssh -o BatchMode=yes "$TESTBOX" "set -euo pipefail
cd '$REMOTE_REPO_DIR'
REMOTE_RUN='$REMOTE_RUN' \
COMPOSE_PROJECT='$COMPOSE_PROJECT' \
HA_RUNTIME_DIR='$REMOTE_RUNTIME_DIR' \
KEEP_REMOTE_RUN_ON_SUCCESS='$KEEP_REMOTE_RUN_ON_SUCCESS' \
bash tests/ha/scripts/run_testbox_ha_suite.sh
" > /tmp/run-ha-testbox-suite.stdout 2> /tmp/run-ha-testbox-suite.stderr
STATUS="$?"
set -e

mkdir -p "$ROOT_DIR/.tmp"
LOCAL_RESULT_DIR="$ROOT_DIR/.tmp/ha-testbox-${RUN_ID}"
mkdir -p "$LOCAL_RESULT_DIR"
rsync -az "$TESTBOX:$REMOTE_RUN/" "$LOCAL_RESULT_DIR/"

cat /tmp/run-ha-testbox-suite.stdout
if [[ -s /tmp/run-ha-testbox-suite.stderr ]]; then
  cat /tmp/run-ha-testbox-suite.stderr >&2
fi
rm -f /tmp/run-ha-testbox-suite.stdout /tmp/run-ha-testbox-suite.stderr

echo "REMOTE_RUN=$REMOTE_RUN"
echo "LOCAL_RESULT_DIR=$LOCAL_RESULT_DIR"

if [[ "$STATUS" -eq 0 && "$KEEP_REMOTE_RUN_ON_SUCCESS" != "true" ]]; then
  ssh -o BatchMode=yes "$TESTBOX" "rm -rf '$REMOTE_RUN'"
fi

exit "$STATUS"
