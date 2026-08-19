#!/usr/bin/env bash
set -euo pipefail
umask 077

show_help() {
  cat <<'EOF'
Usage: scripts/run-performance-recovery-testbox-comparison.sh

Export a read-only core/observability snapshot from 101, then compare a baseline and the current
candidate in isolated internal-network testbox containers. The script removes only its own source
staging directory and REMOTE_RUN after collecting the non-sensitive comparison summary.

Optional environment:
  BASELINE_REF    Baseline Git revision, defaults to the initiative baseline
  DURATION_SECS   Per-variant duration, defaults to 600
  RUN_ID          Explicit unique testbox run id
EOF
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  show_help
  exit 0
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BASELINE_REF="${BASELINE_REF:-1d6d93cbf4de6e673d75811fadd21f45b9a40482}"
DURATION_SECS="${DURATION_SECS:-600}"
TESTBOX_HOST="${TESTBOX_HOST:-codex-testbox}"
RUN_ID="${RUN_ID:-$(date -u +%Y%m%d_%H%M%S)_$(git -C "$ROOT_DIR" rev-parse --short HEAD)_recovery_compare}"

[[ "$RUN_ID" =~ ^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$ ]] || { echo "invalid RUN_ID" >&2; exit 2; }
[[ "$DURATION_SECS" =~ ^[0-9]+$ ]] && (( DURATION_SECS >= 60 )) || {
  echo "DURATION_SECS must be at least 60" >&2
  exit 2
}
git -C "$ROOT_DIR" rev-parse --verify "${BASELINE_REF}^{commit}" >/dev/null

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/tavily-hikari-recovery.XXXXXX")"
TESTBOX_OUTPUT="$TMP_DIR/testbox-comparison.log"
REMOTE_RUN=""
COMPOSE_PROJECT=""
completed=false
collect_sanitized_summaries() {
  local destination="$TMP_DIR/result"
  mkdir -p "$destination"
  for variant in baseline candidate; do
    local remote_summary="$REMOTE_RUN/artifacts/performance-recovery/$variant/summary.json"
    if ssh -o BatchMode=yes "$TESTBOX_HOST" "test -f '$remote_summary'"; then
      rsync -az "$TESTBOX_HOST:$remote_summary" "$destination/$variant-summary.json"
      printf '%s summary:\n' "$variant"
      cat "$destination/$variant-summary.json"
    fi
  done
}
cleanup() {
  rm -rf "$TMP_DIR"
  if [[ "$completed" != true && -n "$REMOTE_RUN" ]]; then
    ssh -o BatchMode=yes "$TESTBOX_HOST" \
      "docker compose -p '$COMPOSE_PROJECT' -f '$REMOTE_RUN/performance-recovery/compose.yml' down -v --remove-orphans >/dev/null 2>&1 || true; rm -rf '$REMOTE_RUN'" \
      >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

echo "Exporting the read-only 101 dual-database snapshot..."
snapshot_output="$(RUN_ID="$RUN_ID" "$ROOT_DIR/scripts/export-live-db-snapshot-to-testbox.sh")"
printf '%s\n' "$snapshot_output"
REMOTE_RUN="$(printf '%s\n' "$snapshot_output" | awk -F= '/^REMOTE_RUN=/{print $2; exit}')"
[[ "$REMOTE_RUN" =~ ^/srv/codex/workspaces/.+/runs/[A-Za-z0-9_.-]+$ ]] || {
  echo "snapshot export did not return a safe REMOTE_RUN" >&2
  exit 2
}

echo "Preparing baseline source at ${BASELINE_REF}..."
BASELINE_ARCHIVE="$TMP_DIR/baseline-source.tar"
git -C "$ROOT_DIR" archive --output="$BASELINE_ARCHIVE" "$BASELINE_REF"
tar -xf "$BASELINE_ARCHIVE" -C "$TMP_DIR"
rm -f "$BASELINE_ARCHIVE"
ssh -o BatchMode=yes "$TESTBOX_HOST" "mkdir -p '$REMOTE_RUN/baseline-repo' && chmod 700 '$REMOTE_RUN/baseline-repo'"
rsync -az --delete --exclude '.git/' "$TMP_DIR/" "$TESTBOX_HOST:$REMOTE_RUN/baseline-repo/"

COMPOSE_PROJECT="$(python3 - "$RUN_ID" <<'PY'
import re
import sys
value = re.sub(r'[^a-z0-9_-]+', '_', f'codex_recovery_{sys.argv[1]}'.lower()).strip('_')
print(value[:63])
PY
)"

echo "Running isolated baseline/candidate comparison on codex-testbox..."
if ssh -o BatchMode=yes "$TESTBOX_HOST" "set -euo pipefail
REMOTE_RUN='$REMOTE_RUN' \\
CANDIDATE_REPO='$REMOTE_RUN/repo' \\
BASELINE_REPO='$REMOTE_RUN/baseline-repo' \\
SNAPSHOT_DIR='$REMOTE_RUN/live-db' \\
COMPOSE_PROJECT='$COMPOSE_PROJECT' \\
DURATION_SECS='$DURATION_SECS' \\
bash '$REMOTE_RUN/repo/tests/performance_recovery/run_snapshot_comparison.sh'
" >"$TESTBOX_OUTPUT" 2>&1; then
  :
else
  testbox_status=$?
  echo "Collecting sanitized partial summaries from the failed comparison..." >&2
  collect_sanitized_summaries >&2 || true
  tail -120 "$TESTBOX_OUTPUT" >&2 || true
  exit "$testbox_status"
fi

echo "Collecting sanitized comparison summary..."
mkdir -p "$TMP_DIR/result"
rsync -az "$TESTBOX_HOST:$REMOTE_RUN/artifacts/performance-recovery/comparison.json" "$TMP_DIR/result/comparison.json"
cat "$TMP_DIR/result/comparison.json"
echo "Cleaning isolated codex-testbox run..."
ssh -o BatchMode=yes "$TESTBOX_HOST" "rm -rf '$REMOTE_RUN' && test ! -e '$REMOTE_RUN'"
completed=true
echo "Performance recovery comparison passed and temporary snapshots were removed."
