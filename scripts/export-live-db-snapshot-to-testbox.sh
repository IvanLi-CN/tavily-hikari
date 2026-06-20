#!/usr/bin/env bash
set -euo pipefail

show_help() {
  cat <<'EOF'
Usage: scripts/export-live-db-snapshot-to-testbox.sh

Create a full read-only SQLite snapshot set on machine 101 and upload it into an isolated
codex-testbox run directory for offline validation.

Environment variables:
  SOURCE_HOST                 Defaults to 192.168.31.11
  SOURCE_SSH_TARGET           Defaults to SOURCE_HOST
  TESTBOX_HOST                Defaults to codex-testbox
  SOURCE_CONTAINER_NAME       Defaults to tavily-hikari
  SOURCE_CONTAINER_DB_DIR     Defaults to /srv/app/data
  SOURCE_HELPER_IMAGE         Defaults to python:3.12-alpine
  SOURCE_BACKUP_PAGES         Defaults to -1 (single-step backup to avoid hot-DB restart loops)
  SOURCE_BACKUP_SLEEP_SECS    Defaults to 0.005
  SOURCE_BACKUP_PROGRESS_SECS Defaults to 15
  SOURCE_DB_DIR               Fallback host path, defaults to /var/lib/docker/volumes/ai-tavily-hikari-data/_data
  SOURCE_CORE_DB_NAME         Defaults to tavily_proxy.db
  SOURCE_OBSERVABILITY_DB_NAME Defaults to tavily_proxy-observability.db
  SOURCE_SNAPSHOT_DIR         Defaults to /home/ivan/srv/media/shared_data/<repo>-<run-id>
  RUN_ID                      Optional explicit run id
  KEEP_SOURCE_SNAPSHOTS       true/false, defaults to false

Outputs:
  Prints the remote run directory and writes manifests under:
    <remote-run>/live-db/manifest.env
    <remote-run>/live-db/sha256sums.txt
EOF
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  show_help
  exit 0
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE_HOST="${SOURCE_HOST:-192.168.31.11}"
SOURCE_SSH_TARGET="${SOURCE_SSH_TARGET:-$SOURCE_HOST}"
TESTBOX_HOST="${TESTBOX_HOST:-codex-testbox}"
SOURCE_CONTAINER_NAME="${SOURCE_CONTAINER_NAME:-tavily-hikari}"
SOURCE_CONTAINER_DB_DIR="${SOURCE_CONTAINER_DB_DIR:-/srv/app/data}"
SOURCE_HELPER_IMAGE="${SOURCE_HELPER_IMAGE:-python:3.12-alpine}"
SOURCE_BACKUP_PAGES="${SOURCE_BACKUP_PAGES:--1}"
SOURCE_BACKUP_SLEEP_SECS="${SOURCE_BACKUP_SLEEP_SECS:-0.005}"
SOURCE_BACKUP_PROGRESS_SECS="${SOURCE_BACKUP_PROGRESS_SECS:-15}"
SOURCE_DB_DIR="${SOURCE_DB_DIR:-/var/lib/docker/volumes/ai-tavily-hikari-data/_data}"
SOURCE_CORE_DB_NAME="${SOURCE_CORE_DB_NAME:-tavily_proxy.db}"
SOURCE_OBSERVABILITY_DB_NAME="${SOURCE_OBSERVABILITY_DB_NAME:-tavily_proxy-observability.db}"
KEEP_SOURCE_SNAPSHOTS="${KEEP_SOURCE_SNAPSHOTS:-false}"

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
RUN_ID="${RUN_ID:-$(date -u +%Y%m%d_%H%M%S)_${GIT_SHA}_ha_outbox}"
WORKSPACE_SLUG="${REPO_NAME}__${PATH_HASH8}"
REMOTE_BASE="/srv/codex/workspaces/$USER"
REMOTE_WORKSPACE="$REMOTE_BASE/$WORKSPACE_SLUG"
REMOTE_RUN="$REMOTE_WORKSPACE/runs/$RUN_ID"
REMOTE_REPO_DIR="$REMOTE_RUN/repo"
REMOTE_DB_DIR="$REMOTE_RUN/live-db"

SOURCE_TMP_DIR="${SOURCE_SNAPSHOT_DIR:-/home/ivan/srv/media/shared_data/${REPO_NAME}-${RUN_ID}}"
SOURCE_CORE_LIVE="$SOURCE_DB_DIR/$SOURCE_CORE_DB_NAME"
SOURCE_SIDECAR_LIVE="$SOURCE_DB_DIR/$SOURCE_OBSERVABILITY_DB_NAME"
SOURCE_CORE_SNAPSHOT="$SOURCE_TMP_DIR/$SOURCE_CORE_DB_NAME"
SOURCE_SIDECAR_SNAPSHOT="$SOURCE_TMP_DIR/$SOURCE_OBSERVABILITY_DB_NAME"

manifest_get() {
  local key="$1"
  printf '%s\n' "$SOURCE_MANIFEST" | awk -F= -v target="$key" '$1 == target { sub($1"=",""); print; exit }'
}

printf 'Preparing isolated codex-testbox run dir: %s\n' "$REMOTE_RUN"
ssh -o BatchMode=yes "$TESTBOX_HOST" "mkdir -p '$REMOTE_DB_DIR' '$REMOTE_REPO_DIR' '$REMOTE_WORKSPACE'"

CREATED_UTC="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
ssh -o BatchMode=yes "$TESTBOX_HOST" "cat > '$REMOTE_WORKSPACE/workspace.txt'" <<TXT
local_repo_root=$REPO_ROOT
created_utc=$CREATED_UTC
source_host=$SOURCE_HOST
source_container_name=$SOURCE_CONTAINER_NAME
source_container_db_dir=$SOURCE_CONTAINER_DB_DIR
source_helper_image=$SOURCE_HELPER_IMAGE
source_backup_pages=$SOURCE_BACKUP_PAGES
source_backup_sleep_secs=$SOURCE_BACKUP_SLEEP_SECS
source_backup_progress_secs=$SOURCE_BACKUP_PROGRESS_SECS
source_db_dir=$SOURCE_DB_DIR
TXT

printf 'Syncing repo to codex-testbox run dir...\n'
rsync -az --delete \
  --exclude '.git/' \
  --exclude 'node_modules/' \
  --exclude 'target/' \
  --exclude 'dist/' \
  --exclude 'build/' \
  --exclude '.next/' \
  --exclude '.venv/' \
  --exclude '*.db' \
  --exclude '*.db-*' \
  "$REPO_ROOT/" "$TESTBOX_HOST:$REMOTE_REPO_DIR/"

printf 'Creating read-only SQLite backups on %s ...\n' "$SOURCE_SSH_TARGET"
SOURCE_MANIFEST="$(ssh -o BatchMode=yes "$SOURCE_SSH_TARGET" "bash -s" -- \
  "$SOURCE_TMP_DIR" \
  "$SOURCE_CONTAINER_NAME" \
  "$SOURCE_CONTAINER_DB_DIR" \
  "$SOURCE_HELPER_IMAGE" \
  "$SOURCE_BACKUP_PAGES" \
  "$SOURCE_BACKUP_SLEEP_SECS" \
  "$SOURCE_BACKUP_PROGRESS_SECS" \
  "$SOURCE_CORE_LIVE" \
  "$SOURCE_SIDECAR_LIVE" \
  "$SOURCE_CORE_SNAPSHOT" \
  "$SOURCE_SIDECAR_SNAPSHOT" <<'EOS'
set -euo pipefail

tmp_dir="$1"
container_name="$2"
container_db_dir="$3"
helper_image="$4"
backup_pages="$5"
backup_sleep_secs="$6"
backup_progress_secs="$7"
core_live="$8"
sidecar_live="$9"
core_snapshot="${10}"
sidecar_snapshot="${11}"

mkdir -p "$tmp_dir"
available_tmp_bytes="$(df -B1 --output=avail "$tmp_dir" | tail -n1 | tr -d ' ')"

snapshot_source_kind="host-path"
effective_core_live="$core_live"
effective_sidecar_live="$sidecar_live"

if docker inspect "$container_name" >/dev/null 2>&1; then
  if docker exec "$container_name" sh -lc "command -v sqlite3 >/dev/null && test -f '$container_db_dir/$(basename "$core_live")' && test -f '$container_db_dir/$(basename "$sidecar_live")'"; then
    snapshot_source_kind="container-sqlite-backup"
    effective_core_live="$container_db_dir/$(basename "$core_live")"
    effective_sidecar_live="$container_db_dir/$(basename "$sidecar_live")"
    core_live_bytes="$(docker exec "$container_name" sh -lc "stat -c %s '$effective_core_live'")"
    core_live_wal_bytes="$(docker exec "$container_name" sh -lc "stat -c %s '${effective_core_live}-wal' 2>/dev/null || echo 0")"
    sidecar_live_bytes="$(docker exec "$container_name" sh -lc "stat -c %s '$effective_sidecar_live'")"
  elif docker exec "$container_name" sh -lc "test -f '$container_db_dir/$(basename "$core_live")' && test -f '$container_db_dir/$(basename "$sidecar_live")'"; then
    snapshot_source_kind="container-helper-python-backup"
    effective_core_live="$container_db_dir/$(basename "$core_live")"
    effective_sidecar_live="$container_db_dir/$(basename "$sidecar_live")"
    core_live_bytes="$(docker run --rm --volumes-from "$container_name":ro "$helper_image" sh -lc "stat -c %s '$effective_core_live'")"
    core_live_wal_bytes="$(docker run --rm --volumes-from "$container_name":ro "$helper_image" sh -lc "stat -c %s '${effective_core_live}-wal' 2>/dev/null || echo 0")"
    sidecar_live_bytes="$(docker run --rm --volumes-from "$container_name":ro "$helper_image" sh -lc "stat -c %s '$effective_sidecar_live'")"
  fi
fi

if [[ "$snapshot_source_kind" == "host-path" ]]; then
  test -f "$effective_core_live"
  test -f "$effective_sidecar_live"
  core_live_bytes="$(stat -c %s "$effective_core_live")"
  core_live_wal_bytes="$(stat -c %s "${effective_core_live}-wal" 2>/dev/null || echo 0)"
  sidecar_live_bytes="$(stat -c %s "$effective_sidecar_live")"
fi

required_tmp_bytes="$((core_live_bytes + core_live_wal_bytes + sidecar_live_bytes + 1073741824))"

if (( available_tmp_bytes < required_tmp_bytes )); then
  echo "insufficient temporary free space for snapshot: available=${available_tmp_bytes} required=${required_tmp_bytes}" >&2
  exit 2
fi

rm -f "$core_snapshot" "$sidecar_snapshot"

if [[ "$snapshot_source_kind" == "container-sqlite-backup" ]]; then
  docker exec "$container_name" sh -lc "sqlite3 '$effective_core_live' \".timeout 10000\" \".backup '/tmp/$(basename "$core_snapshot")'\""
  docker exec "$container_name" sh -lc "sqlite3 '$effective_sidecar_live' \".timeout 10000\" \".backup '/tmp/$(basename "$sidecar_snapshot")'\""
  docker cp "$container_name:/tmp/$(basename "$core_snapshot")" "$core_snapshot"
  docker cp "$container_name:/tmp/$(basename "$sidecar_snapshot")" "$sidecar_snapshot"
  docker exec "$container_name" sh -lc "rm -f '/tmp/$(basename "$core_snapshot")' '/tmp/$(basename "$sidecar_snapshot")'"
elif [[ "$snapshot_source_kind" == "container-helper-python-backup" ]]; then
  docker run --rm \
    --volumes-from "$container_name":ro \
    -v "$tmp_dir:/backup" \
    "$helper_image" \
    python3 -c '
import os
import sqlite3
import sys
import time


BACKUP_PAGES = int(sys.argv[1])
BACKUP_SLEEP_SECS = float(sys.argv[2])
BACKUP_PROGRESS_SECS = float(sys.argv[3])
CORE_SRC_PATH = sys.argv[4]
CORE_DST_PATH = sys.argv[5]
SIDECAR_SRC_PATH = sys.argv[6]
SIDECAR_DST_PATH = sys.argv[7]


def backup_database(label: str, src_path: str, dst_path: str) -> None:
    if os.path.exists(dst_path):
        os.remove(dst_path)
    src = sqlite3.connect(f"file:{src_path}?mode=ro", uri=True, timeout=30.0)
    dst = sqlite3.connect(dst_path, timeout=30.0)
    start = time.time()
    last_report = 0.0

    def progress(status: int, remaining: int, total: int) -> None:
        nonlocal last_report
        now = time.time()
        if total <= 0:
            return
        if remaining == 0 or last_report == 0.0 or (now - last_report) >= BACKUP_PROGRESS_SECS:
            copied = total - remaining
            pct = (copied / total) * 100.0
            elapsed = now - start
            print(
                f"[sqlite-backup] label={label} copied_pages={copied} total_pages={total} remaining_pages={remaining} pct={pct:.2f} elapsed_secs={elapsed:.1f}",
                file=sys.stderr,
                flush=True,
            )
            last_report = now

    try:
        dst.execute("PRAGMA journal_mode=OFF;")
        dst.execute("PRAGMA synchronous=OFF;")
        dst.execute("PRAGMA temp_store=MEMORY;")
        dst.execute("PRAGMA locking_mode=EXCLUSIVE;")
        if BACKUP_PAGES == -1:
            total_pages = src.execute("PRAGMA page_count;").fetchone()[0]
            print(
                f"[sqlite-backup] label={label} mode=single-step total_pages={total_pages}",
                file=sys.stderr,
                flush=True,
            )
        src.backup(
            dst,
            pages=BACKUP_PAGES,
            sleep=BACKUP_SLEEP_SECS,
            progress=progress,
        )
        dst.commit()
    finally:
        src.close()
        dst.close()


backup_database("core", CORE_SRC_PATH, CORE_DST_PATH)
backup_database("observability", SIDECAR_SRC_PATH, SIDECAR_DST_PATH)
' "$backup_pages" "$backup_sleep_secs" "$backup_progress_secs" "$effective_core_live" "/backup/$(basename "$core_snapshot")" "$effective_sidecar_live" "/backup/$(basename "$sidecar_snapshot")"
else
  sqlite3 "$effective_core_live" ".timeout 10000" ".backup '$core_snapshot'"
  sqlite3 "$effective_sidecar_live" ".timeout 10000" ".backup '$sidecar_snapshot'"
fi

core_integrity="$(sqlite3 "$core_snapshot" 'PRAGMA integrity_check;' | tr -d '\r')"
sidecar_integrity="$(sqlite3 "$sidecar_snapshot" 'PRAGMA integrity_check;' | tr -d '\r')"
core_snapshot_bytes="$(stat -c %s "$core_snapshot")"
sidecar_snapshot_bytes="$(stat -c %s "$sidecar_snapshot")"
core_snapshot_page_count="$(sqlite3 "$core_snapshot" 'PRAGMA page_count;' | tr -d '\r')"
sidecar_snapshot_page_count="$(sqlite3 "$sidecar_snapshot" 'PRAGMA page_count;' | tr -d '\r')"

if (( core_snapshot_bytes <= 0 )); then
  echo "core snapshot is empty" >&2
  exit 3
fi
if (( sidecar_snapshot_bytes <= 0 )); then
  echo "sidecar snapshot is empty" >&2
  exit 4
fi
if [[ "${core_snapshot_page_count:-0}" == "0" ]]; then
  echo "core snapshot page_count is zero" >&2
  exit 5
fi
if [[ "${sidecar_snapshot_page_count:-0}" == "0" ]]; then
  echo "sidecar snapshot page_count is zero" >&2
  exit 6
fi

if [[ "$core_integrity" != "ok" ]]; then
  echo "core snapshot integrity_check failed: $core_integrity" >&2
  exit 7
fi
if [[ "$sidecar_integrity" != "ok" ]]; then
  echo "sidecar snapshot integrity_check failed: $sidecar_integrity" >&2
  exit 8
fi

printf 'source_tmp_dir=%s\n' "$tmp_dir"
printf 'snapshot_source_kind=%s\n' "$snapshot_source_kind"
printf 'helper_image=%s\n' "$helper_image"
printf 'core_live_path=%s\n' "$effective_core_live"
printf 'sidecar_live_path=%s\n' "$effective_sidecar_live"
printf 'core_live_bytes=%s\n' "$core_live_bytes"
printf 'core_live_wal_bytes=%s\n' "$core_live_wal_bytes"
printf 'sidecar_live_bytes=%s\n' "$sidecar_live_bytes"
printf 'available_tmp_bytes=%s\n' "$available_tmp_bytes"
printf 'required_tmp_bytes=%s\n' "$required_tmp_bytes"
printf 'core_snapshot_path=%s\n' "$core_snapshot"
printf 'sidecar_snapshot_path=%s\n' "$sidecar_snapshot"
printf 'core_snapshot_bytes=%s\n' "$core_snapshot_bytes"
printf 'sidecar_snapshot_bytes=%s\n' "$sidecar_snapshot_bytes"
printf 'core_snapshot_page_count=%s\n' "$core_snapshot_page_count"
printf 'sidecar_snapshot_page_count=%s\n' "$sidecar_snapshot_page_count"
printf 'core_snapshot_sha256=%s\n' "$(sha256sum "$core_snapshot" | awk '{print $1}')"
printf 'sidecar_snapshot_sha256=%s\n' "$(sha256sum "$sidecar_snapshot" | awk '{print $1}')"
printf 'core_snapshot_integrity=%s\n' "$core_integrity"
printf 'sidecar_snapshot_integrity=%s\n' "$sidecar_integrity"
EOS
)"

SOURCE_TMP_DIR_REMOTE="$(manifest_get source_tmp_dir)"
SNAPSHOT_SOURCE_KIND="$(manifest_get snapshot_source_kind)"
HELPER_IMAGE_REMOTE="$(manifest_get helper_image)"
CORE_LIVE_PATH_REMOTE="$(manifest_get core_live_path)"
SIDECAR_LIVE_PATH_REMOTE="$(manifest_get sidecar_live_path)"
CORE_LIVE_BYTES="$(manifest_get core_live_bytes)"
CORE_LIVE_WAL_BYTES="$(manifest_get core_live_wal_bytes)"
SIDECAR_LIVE_BYTES="$(manifest_get sidecar_live_bytes)"
AVAILABLE_TMP_BYTES="$(manifest_get available_tmp_bytes)"
REQUIRED_TMP_BYTES="$(manifest_get required_tmp_bytes)"
CORE_SNAPSHOT_PATH_REMOTE="$(manifest_get core_snapshot_path)"
SIDECAR_SNAPSHOT_PATH_REMOTE="$(manifest_get sidecar_snapshot_path)"
CORE_SNAPSHOT_BYTES="$(manifest_get core_snapshot_bytes)"
SIDECAR_SNAPSHOT_BYTES="$(manifest_get sidecar_snapshot_bytes)"
CORE_SNAPSHOT_SHA256="$(manifest_get core_snapshot_sha256)"
SIDECAR_SNAPSHOT_SHA256="$(manifest_get sidecar_snapshot_sha256)"
CORE_SNAPSHOT_INTEGRITY="$(manifest_get core_snapshot_integrity)"
SIDECAR_SNAPSHOT_INTEGRITY="$(manifest_get sidecar_snapshot_integrity)"

printf 'Streaming full snapshot set to codex-testbox ...\n'
ssh -o BatchMode=yes "$SOURCE_SSH_TARGET" "cat '$CORE_SNAPSHOT_PATH_REMOTE'" \
  | ssh -o BatchMode=yes "$TESTBOX_HOST" "cat > '$REMOTE_DB_DIR/$SOURCE_CORE_DB_NAME'"
ssh -o BatchMode=yes "$SOURCE_SSH_TARGET" "cat '$SIDECAR_SNAPSHOT_PATH_REMOTE'" \
  | ssh -o BatchMode=yes "$TESTBOX_HOST" "cat > '$REMOTE_DB_DIR/$SOURCE_OBSERVABILITY_DB_NAME'"

ssh -o BatchMode=yes "$TESTBOX_HOST" "cat > '$REMOTE_DB_DIR/manifest.env'" <<EOF
run_id=$RUN_ID
created_utc=$CREATED_UTC
source_host=$SOURCE_HOST
source_ssh_target=$SOURCE_SSH_TARGET
source_container_name=$SOURCE_CONTAINER_NAME
source_container_db_dir=$SOURCE_CONTAINER_DB_DIR
source_helper_image=$SOURCE_HELPER_IMAGE
source_backup_pages=$SOURCE_BACKUP_PAGES
source_backup_sleep_secs=$SOURCE_BACKUP_SLEEP_SECS
source_backup_progress_secs=$SOURCE_BACKUP_PROGRESS_SECS
source_db_dir=$SOURCE_DB_DIR
snapshot_source_kind=$SNAPSHOT_SOURCE_KIND
helper_image=$HELPER_IMAGE_REMOTE
core_live_path=$CORE_LIVE_PATH_REMOTE
sidecar_live_path=$SIDECAR_LIVE_PATH_REMOTE
core_live_bytes=$CORE_LIVE_BYTES
core_live_wal_bytes=$CORE_LIVE_WAL_BYTES
sidecar_live_bytes=$SIDECAR_LIVE_BYTES
available_tmp_bytes=$AVAILABLE_TMP_BYTES
required_tmp_bytes=$REQUIRED_TMP_BYTES
core_snapshot_bytes=$CORE_SNAPSHOT_BYTES
sidecar_snapshot_bytes=$SIDECAR_SNAPSHOT_BYTES
core_snapshot_sha256=$CORE_SNAPSHOT_SHA256
sidecar_snapshot_sha256=$SIDECAR_SNAPSHOT_SHA256
core_snapshot_integrity=$CORE_SNAPSHOT_INTEGRITY
sidecar_snapshot_integrity=$SIDECAR_SNAPSHOT_INTEGRITY
remote_run=$REMOTE_RUN
remote_repo_dir=$REMOTE_REPO_DIR
remote_db_dir=$REMOTE_DB_DIR
EOF

ssh -o BatchMode=yes "$TESTBOX_HOST" "cat > '$REMOTE_DB_DIR/sha256sums.txt'" <<EOF
$CORE_SNAPSHOT_SHA256  $SOURCE_CORE_DB_NAME
$SIDECAR_SNAPSHOT_SHA256  $SOURCE_OBSERVABILITY_DB_NAME
EOF

printf 'Verifying uploaded files on codex-testbox ...\n'
ssh -o BatchMode=yes "$TESTBOX_HOST" "cd '$REMOTE_DB_DIR' \
  && sha256sum -c sha256sums.txt \
  && test \"\$(sqlite3 '$SOURCE_CORE_DB_NAME' 'PRAGMA integrity_check;')\" = ok \
  && test \"\$(sqlite3 '$SOURCE_OBSERVABILITY_DB_NAME' 'PRAGMA integrity_check;')\" = ok"

if [[ "$KEEP_SOURCE_SNAPSHOTS" != "true" && "$KEEP_SOURCE_SNAPSHOTS" != "1" ]]; then
  printf 'Cleaning temporary backups on %s ...\n' "$SOURCE_SSH_TARGET"
  ssh -o BatchMode=yes "$SOURCE_SSH_TARGET" "rm -f '$CORE_SNAPSHOT_PATH_REMOTE' '$SIDECAR_SNAPSHOT_PATH_REMOTE' && rmdir '$SOURCE_TMP_DIR_REMOTE' 2>/dev/null || true"
fi

printf '\nSnapshot export complete.\n'
printf 'REMOTE_RUN=%s\n' "$REMOTE_RUN"
printf 'REMOTE_REPO_DIR=%s\n' "$REMOTE_REPO_DIR"
printf 'REMOTE_DB_DIR=%s\n' "$REMOTE_DB_DIR"
printf 'CORE_SHA256=%s\n' "$CORE_SNAPSHOT_SHA256"
printf 'SIDECAR_SHA256=%s\n' "$SIDECAR_SNAPSHOT_SHA256"
