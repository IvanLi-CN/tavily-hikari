#!/usr/bin/env bash
set -euo pipefail
umask 077

show_help() {
  cat <<'EOF'
Usage: scripts/run-performance-recovery-github-hosted.sh

Run the manual performance recovery comparison entirely on a GitHub-hosted runner. The script
generates disposable SQLite/core-sidecar fixtures from the candidate build, stages both source
revisions locally, and delegates the isolated Docker comparison to run_snapshot_comparison.sh.

Optional environment:
  BASELINE_REF    Baseline Git revision, defaults to the initiative baseline
  CANDIDATE_REF   Candidate Git revision, defaults to HEAD
  DURATION_SECS   Per-variant duration, defaults to 600
  RUN_ID          Safe identifier for the temporary local run
EOF
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  show_help
  exit 0
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BASELINE_REF="${BASELINE_REF:-1d6d93cbf4de6e673d75811fadd21f45b9a40482}"
CANDIDATE_REF="${CANDIDATE_REF:-HEAD}"
DURATION_SECS="${DURATION_SECS:-600}"
RUN_ID="${RUN_ID:-$(date -u +%Y%m%d_%H%M%S)_github_hosted_recovery}"
RUNNER_TEMP_ROOT="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
RUN_ROOT="$RUNNER_TEMP_ROOT/performance-recovery-local/$RUN_ID"
EVIDENCE_DIR="$RUNNER_TEMP_ROOT/performance-recovery"
SNAPSHOT_DIR="$RUN_ROOT/snapshot"
CANDIDATE_REPO="$RUN_ROOT/repo"
BASELINE_REPO="$RUN_ROOT/baseline-repo"
COMPOSE_PROJECT="github_hosted_recovery_${GITHUB_RUN_ID:-$$}"
FIXTURE_IMAGE="performance-recovery-fixture:${GITHUB_RUN_ID:-local}"
FIXTURE_CONTAINER="performance-recovery-fixture-${GITHUB_RUN_ID:-$$}"
RUST_BASE_IMAGE="rust:1.91-bookworm@sha256:c1e5f19e773b7878c3f7a805dd00a495e747acbdc76fb2337a4ebf0418896b33"

[[ "$RUN_ID" =~ ^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$ ]] || {
  echo "invalid RUN_ID" >&2
  exit 2
}
if ! [[ "$DURATION_SECS" =~ ^[0-9]+$ ]] ||
  ! (( DURATION_SECS >= 600 && DURATION_SECS <= 1800 )); then
  echo "DURATION_SECS must be an integer between 600 and 1800" >&2
  exit 2
fi

for command in git docker python3 zstd sha256sum curl tar; do
  command -v "$command" >/dev/null || {
    echo "missing required runner command: $command" >&2
    exit 2
  }
done
docker info >/dev/null

RUNNER_UID="$(id -u)"
RUNNER_GID="$(id -g)"

CANDIDATE_SHA="$(git -C "$ROOT_DIR" rev-parse --verify "${CANDIDATE_REF}^{commit}")"
BASELINE_SHA="$(git -C "$ROOT_DIR" rev-parse --verify "${BASELINE_REF}^{commit}")"
[[ "$BASELINE_SHA" != "$CANDIDATE_SHA" ]] || {
  echo "baseline_ref and candidate_ref must resolve to different commits" >&2
  exit 2
}

mkdir -p "$SNAPSHOT_DIR" "$CANDIDATE_REPO" "$BASELINE_REPO" \
  "$RUN_ROOT/fixture-data" "$EVIDENCE_DIR"

cleanup() {
  if [[ -f "$RUN_ROOT/performance-recovery/compose.yml" ]]; then
    docker compose -p "$COMPOSE_PROJECT" \
      -f "$RUN_ROOT/performance-recovery/compose.yml" down -v --remove-orphans \
      >/dev/null 2>&1 || true
  fi
  docker rm -f "$FIXTURE_CONTAINER" >/dev/null 2>&1 || true
  docker image rm -f "$FIXTURE_IMAGE" >/dev/null 2>&1 || true
  rm -rf -- "$RUN_ROOT"
}
trap cleanup EXIT

archive_repo() {
  local ref="$1"
  local destination="$2"
  git -C "$ROOT_DIR" archive --format=tar "$ref" | tar -xf - -C "$destination"
}

archive_repo "$CANDIDATE_SHA" "$CANDIDATE_REPO"
archive_repo "$BASELINE_SHA" "$BASELINE_REPO"

echo "Building the candidate fixture initializer image..."
sed "s|^FROM rust:1.91-bookworm AS builder$|FROM $RUST_BASE_IMAGE AS builder|" \
  "$CANDIDATE_REPO/tests/ha/Dockerfile.app" \
  > "$RUN_ROOT/Dockerfile.fixture"
grep -qx "FROM $RUST_BASE_IMAGE AS builder" "$RUN_ROOT/Dockerfile.fixture" || {
  echo "unexpected fixture Dockerfile base image" >&2
  exit 2
}
docker build \
  --file "$RUN_ROOT/Dockerfile.fixture" \
  --tag "$FIXTURE_IMAGE" \
  "$CANDIDATE_REPO"

echo "Initializing disposable GitHub-hosted SQLite fixtures..."
docker run --detach \
  --name "$FIXTURE_CONTAINER" \
  --user "${RUNNER_UID}:${RUNNER_GID}" \
  --publish 127.0.0.1::8787 \
  --volume "$RUN_ROOT/fixture-data:/srv/app/data" \
  --env PROXY_DB_PATH=/srv/app/data/tavily_proxy.db \
  --env PROXY_BIND=0.0.0.0 \
  --env PROXY_PORT=8787 \
  --env TAVILY_API_KEYS=tvly-fixture-key \
  --env TAVILY_UPSTREAM=http://127.0.0.1:9/mcp \
  --env TAVILY_USAGE_BASE=http://127.0.0.1:9 \
  --env DEV_OPEN_ADMIN=true \
  --env XRAY_BINARY=/bin/true \
  --env HA_MODE=single \
  --env NODE_ID=github-hosted-fixture \
  "$FIXTURE_IMAGE" >/dev/null

fixture_port="$(docker port "$FIXTURE_CONTAINER" 8787/tcp | sed -n 's/.*://p' | head -n 1)"
fixture_ready=false
for _ in $(seq 1 240); do
  dashboard_status="$(curl -sS --max-time 10 -o /dev/null -w '%{http_code}' \
    "http://127.0.0.1:${fixture_port}/api/dashboard/overview" 2>/dev/null || true)"
  if [[ "$dashboard_status" == 200 ]] &&
    [[ -f "$RUN_ROOT/fixture-data/tavily_proxy.db" ]]; then
    fixture_ready=true
    break
  fi
  if [[ "$(docker inspect -f '{{.State.Running}}' "$FIXTURE_CONTAINER" 2>/dev/null || true)" != true ]]; then
    docker logs "$FIXTURE_CONTAINER" >&2 || true
    echo "fixture initializer exited before creating the database" >&2
    exit 1
  fi
  sleep 1
done
if [[ "$fixture_ready" != true ]]; then
  docker logs "$FIXTURE_CONTAINER" >&2 || true
  echo "timed out waiting for the GitHub-hosted fixture initializer" >&2
  exit 1
fi
docker stop --time 30 "$FIXTURE_CONTAINER" >/dev/null
docker rm "$FIXTURE_CONTAINER" >/dev/null

python3 - "$RUN_ROOT/fixture-data/tavily_proxy.db" \
  "$RUN_ROOT/fixture-data/tavily_proxy-observability.db" <<'PY'
from __future__ import annotations

import sqlite3
import sys
from pathlib import Path

for raw_path in sys.argv[1:]:
    path = Path(raw_path)
    path.parent.mkdir(parents=True, exist_ok=True)
    connection = sqlite3.connect(path)
    connection.execute(
        "CREATE TABLE IF NOT EXISTS github_hosted_fixture_marker "
        "(id INTEGER PRIMARY KEY, created_at INTEGER NOT NULL)"
    )
    connection.execute(
        "INSERT OR IGNORE INTO github_hosted_fixture_marker(id, created_at) VALUES (1, strftime('%s', 'now'))"
    )
    connection.commit()
    connection.execute("PRAGMA wal_checkpoint(TRUNCATE)")
    connection.execute("PRAGMA journal_mode=DELETE")
    connection.commit()
    connection.close()
    path.chmod(0o600)
PY

zstd -q -1 --force \
  "$RUN_ROOT/fixture-data/tavily_proxy.db" \
  -o "$SNAPSHOT_DIR/tavily_proxy.db.zst"
zstd -q -1 --force \
  "$RUN_ROOT/fixture-data/tavily_proxy-observability.db" \
  -o "$SNAPSHOT_DIR/tavily_proxy-observability.db.zst"

python3 - "$RUN_ROOT/fixture-data/tavily_proxy.db" \
  "$RUN_ROOT/fixture-data/tavily_proxy-observability.db" \
  "$SNAPSHOT_DIR" <<'PY'
from __future__ import annotations

import hashlib
import sqlite3
import sys
from pathlib import Path

core, sidecar, snapshot_dir = map(Path, sys.argv[1:])


def digest(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def pages(path: Path) -> str:
    with sqlite3.connect(path) as connection:
        return str(connection.execute("PRAGMA page_count").fetchone()[0])


lines = [
    "core_compressed_snapshot_name=tavily_proxy.db.zst",
    "sidecar_compressed_snapshot_name=tavily_proxy-observability.db.zst",
    f"core_snapshot_sha256={digest(core)}",
    f"sidecar_snapshot_sha256={digest(sidecar)}",
    f"core_snapshot_page_count={pages(core)}",
    f"sidecar_snapshot_page_count={pages(sidecar)}",
    f"core_compressed_snapshot_sha256={digest(snapshot_dir / 'tavily_proxy.db.zst')}",
    f"sidecar_compressed_snapshot_sha256={digest(snapshot_dir / 'tavily_proxy-observability.db.zst')}",
    "fixture_kind=github-hosted-local",
]
(snapshot_dir / "manifest.env").write_text("\n".join(lines) + "\n", encoding="utf-8")
(snapshot_dir / "manifest.env").chmod(0o600)
PY

printf 'candidate_sha=%s\nbaseline_sha=%s\nduration_secs=%s\nfixture_kind=github-hosted-local\n' \
  "$CANDIDATE_SHA" "$BASELINE_SHA" "$DURATION_SECS" \
  > "$EVIDENCE_DIR/identity.txt"

echo "Running the isolated GitHub-hosted baseline/candidate comparison..."
set +e
PERFORMANCE_RECOVERY_RUN_MODE=github-hosted \
REMOTE_RUN="$RUN_ROOT" \
CANDIDATE_REPO="$CANDIDATE_REPO" \
BASELINE_REPO="$BASELINE_REPO" \
SNAPSHOT_DIR="$SNAPSHOT_DIR" \
COMPOSE_PROJECT="$COMPOSE_PROJECT" \
CANDIDATE_SHA="$CANDIDATE_SHA" \
BASELINE_SHA="$BASELINE_SHA" \
DURATION_SECS="$DURATION_SECS" \
bash "$ROOT_DIR/tests/performance_recovery/run_snapshot_comparison.sh"
comparison_status=$?
set -e

comparison_artifacts="$RUN_ROOT/artifacts/performance-recovery"
if [[ -f "$comparison_artifacts/comparison.json" ]]; then
  cp "$comparison_artifacts/comparison.json" "$EVIDENCE_DIR/comparison.json"
fi
for variant in baseline candidate; do
  if [[ -f "$comparison_artifacts/$variant/summary.json" ]]; then
    cp "$comparison_artifacts/$variant/summary.json" "$EVIDENCE_DIR/$variant-summary.json"
  fi
done
printf '%s\n' "$comparison_status" > "$EVIDENCE_DIR/exit-status.txt"

if (( comparison_status != 0 )); then
  echo "GitHub-hosted fixture comparison failed; sanitized evidence was copied to $EVIDENCE_DIR" >&2
  exit "$comparison_status"
fi
echo "GitHub-hosted fixture comparison passed. Sanitized evidence is in $EVIDENCE_DIR."
