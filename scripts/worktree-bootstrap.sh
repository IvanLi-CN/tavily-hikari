#!/usr/bin/env bash
set -uo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"

strict=0
force=0
trigger="auto"

usage() {
  cat <<'EOF_USAGE' >&2
Usage: bash scripts/worktree-bootstrap.sh [--hook] [--manual] [--force] [--strict]
EOF_USAGE
}

info() {
  printf '[worktree-bootstrap] %s\n' "$1"
}

warn() {
  printf '[worktree-bootstrap] warning: %s\n' "$1" >&2
}

fail_or_warn() {
  if [ "$strict" -eq 1 ]; then
    printf '[worktree-bootstrap] error: %s\n' "$1" >&2
    exit 1
  fi

  warn "$1"
}

resolve_repo_path() {
  case "$1" in
    /*) printf '%s\n' "$1" ;;
    *) printf '%s/%s\n' "$repo_root" "$1" ;;
  esac
}

is_copyable_env_file() {
  case "$1" in
    .env|.env.*)
      case "$1" in
        .env.example|.env.sample|.env.template|.env.dist|.env.*.example|.env.*.sample|.env.*.template|.env.*.dist)
          return 1
          ;;
        *)
          return 0
          ;;
      esac
      ;;
    *)
      return 1
      ;;
  esac
}

should_restore_bun_deps() {
  [ "$force" -eq 1 ] || [ ! -d "$1/node_modules" ]
}

run_bun_install() {
  local label="$1"
  local dir="$2"

  if [ ! -f "$dir/package.json" ]; then
    warn "skipping $label dependencies because $dir/package.json is missing"
    return 0
  fi

  if ! command -v bun >/dev/null 2>&1; then
    warn "bun not found; skipping $label dependencies"
    return 0
  fi

  info "installing $label dependencies"
  if ! (
    cd "$dir"
    bun install --frozen-lockfile
  ); then
    fail_or_warn "bun install --frozen-lockfile failed in $dir"
  fi
}

copy_missing_envs() {
  local source_count=0
  local copied_count=0
  local kept_count=0
  local env_path

  while IFS= read -r -d '' env_path; do
    local env_name target_path
    env_name="$(basename "$env_path")"
    if ! is_copyable_env_file "$env_name"; then
      continue
    fi

    source_count=$((source_count + 1))
    target_path="$repo_root/$env_name"
    if [ -e "$target_path" ]; then
      kept_count=$((kept_count + 1))
      continue
    fi

    if cp -p "$env_path" "$target_path"; then
      copied_count=$((copied_count + 1))
    else
      fail_or_warn "failed to copy $env_name from $primary_root"
    fi
  done < <(find "$primary_root" -maxdepth 1 -type f \( -name '.env' -o -name '.env.*' \) -print0 2>/dev/null)

  if [ "$source_count" -eq 0 ]; then
    warn "primary worktree has no copyable .env or .env.* files"
    return 0
  fi

  if [ "$copied_count" -gt 0 ]; then
    info "copied $copied_count missing env file(s) from primary worktree"
  fi

  if [ "$kept_count" -gt 0 ]; then
    info "kept $kept_count existing env file(s) in this worktree"
  fi
}

write_state_marker() {
  if ! mkdir -p "$state_dir"; then
    fail_or_warn "failed to create $state_dir"
    return 0
  fi

  if ! cat > "$state_file" <<EOF_STATE
completed_at=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
trigger=$trigger
primary_root=$primary_root
repo_root=$repo_root
EOF_STATE
  then
    fail_or_warn "failed to write $state_file"
  fi
}

while [ $# -gt 0 ]; do
  case "$1" in
    --hook)
      trigger="hook"
      ;;
    --manual)
      trigger="manual"
      ;;
    --force)
      force=1
      ;;
    --strict)
      strict=1
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage
      exit 2
      ;;
  esac
  shift
done

if ! git_dir_raw="$(git -C "$repo_root" rev-parse --git-dir 2>/dev/null)"; then
  fail_or_warn "unable to resolve git dir for $repo_root"
  exit 0
fi

if ! common_dir_raw="$(git -C "$repo_root" rev-parse --git-common-dir 2>/dev/null)"; then
  fail_or_warn "unable to resolve git common dir for $repo_root"
  exit 0
fi

git_dir="$(resolve_repo_path "$git_dir_raw")"
common_dir="$(resolve_repo_path "$common_dir_raw")"
primary_root="$(cd "$common_dir/.." && pwd)"
state_dir="$repo_root/.tmp"
state_file="$state_dir/worktree-bootstrap.v1.done"

if [ "$git_dir" = "$common_dir" ]; then
  if [ "$trigger" = "manual" ]; then
    info "primary worktree detected; skipping linked-worktree bootstrap"
  fi
  exit 0
fi

if [ "$force" -ne 1 ] && [ -f "$state_file" ]; then
  if [ "$trigger" = "manual" ]; then
    info "bootstrap already completed for this worktree; skipping"
  fi
  exit 0
fi

copy_missing_envs

if should_restore_bun_deps "$repo_root"; then
  run_bun_install "repo" "$repo_root"
fi

if should_restore_bun_deps "$repo_root/web"; then
  run_bun_install "web" "$repo_root/web"
fi

if should_restore_bun_deps "$repo_root/docs-site"; then
  run_bun_install "docs-site" "$repo_root/docs-site"
fi

if command -v cargo >/dev/null 2>&1; then
  info "running cargo fetch --locked"
  if ! (
    cd "$repo_root"
    cargo fetch --locked
  ); then
    fail_or_warn "cargo fetch --locked failed"
  fi
else
  warn "cargo not found; skipping cargo fetch --locked"
fi

write_state_marker

if [ "$trigger" = "manual" ]; then
  info "linked worktree bootstrap completed"
fi
