#!/usr/bin/env bash
set -euo pipefail

managed_marker='# managed by tavily-hikari worktree bootstrap'
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"

resolve_repo_path() {
  case "$1" in
    /*) printf '%s\n' "$1" ;;
    *) printf '%s/%s\n' "$repo_root" "$1" ;;
  esac
}

info() {
  printf '[worktree-hooks] %s\n' "$1"
}

warn() {
  printf '[worktree-hooks] warning: %s\n' "$1" >&2
}

common_dir="$(git -C "$repo_root" rev-parse --git-common-dir)"
common_dir="$(resolve_repo_path "$common_dir")"
hooks_dir="$common_dir/hooks"
post_checkout_hook="$hooks_dir/post-checkout"
post_checkout_chain="$hooks_dir/post-checkout.local"
custom_hooks_path="$(git -C "$repo_root" config --get core.hooksPath || true)"
should_write_post_checkout=1

if [ -n "$custom_hooks_path" ]; then
  warn "core.hooksPath is set to $custom_hooks_path; Git will ignore $hooks_dir unless hooksPath is unset"
fi

mkdir -p "$hooks_dir"

if [ -e "$post_checkout_hook" ] || [ -L "$post_checkout_hook" ]; then
  if ! grep -Fq "$managed_marker" "$post_checkout_hook" 2>/dev/null; then
    if [ -e "$post_checkout_chain" ] || [ -L "$post_checkout_chain" ]; then
      warn "unmanaged post-checkout already exists and $post_checkout_chain is occupied; leaving existing hook untouched"
      should_write_post_checkout=0
    else
      mv "$post_checkout_hook" "$post_checkout_chain"
      info "preserved existing post-checkout hook at $post_checkout_chain"
    fi
  fi
fi

if [ "$should_write_post_checkout" -eq 1 ]; then
  cat > "$post_checkout_hook" <<EOF_HOOK
#!/bin/sh
$managed_marker
repo_root="\$(git rev-parse --show-toplevel 2>/dev/null || printf '')"
[ -n "\$repo_root" ] || exit 0

runner="\$repo_root/scripts/worktree-bootstrap.sh"
if [ -f "\$runner" ]; then
  bash "\$runner" --hook || true
fi

common_dir="\$(git rev-parse --git-common-dir 2>/dev/null || printf '')"
case "\$common_dir" in
  '') exit 0 ;;
  /*) ;;
  *) common_dir="\$repo_root/\$common_dir" ;;
esac

chain="\$common_dir/hooks/post-checkout.local"
[ -x "\$chain" ] || exit 0
exec "\$chain" "\$@"
EOF_HOOK
  chmod +x "$post_checkout_hook"
  info "installed shared post-checkout hook in $hooks_dir"
fi

if command -v lefthook >/dev/null 2>&1; then
  if (cd "$repo_root" && LEFTHOOK=0 lefthook install); then
    info "installed lefthook-managed commit hooks"
  else
    warn "lefthook install failed; post-checkout wrapper is still active"
  fi
else
  warn "lefthook not found; skipping pre-commit/commit-msg installation"
fi
