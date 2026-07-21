#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/tavily-hikari-worktree-bootstrap.XXXXXX")"
tmp_dir="$(cd "$tmp_dir" && pwd)"
trap 'rm -rf "$tmp_dir"' EXIT

copy_repo() {
  local src="$1"
  local dest="$2"
  mkdir -p "$dest"
  rsync -a \
    --exclude '.git' \
    --exclude '.env' \
    --exclude '.env.*' \
    --exclude 'node_modules' \
    --exclude 'web/node_modules' \
    --exclude 'docs-site/node_modules' \
    --exclude 'target' \
    --exclude 'web/dist' \
    --exclude 'web/storybook-static' \
    --exclude 'downloads' \
    --exclude 'data' \
    --exclude '.tmp' \
    "$src/" "$dest/"
}

init_repo() {
  local repo="$1"
  git -C "$repo" init -b main >/dev/null
  git -C "$repo" config user.name 'Codex Test'
  git -C "$repo" config user.email 'codex-test@example.com'
  # Keep the fixture repo fully synchronous so teardown does not race
  # background Git maintenance on CI runners.
  git -C "$repo" config gc.auto 0
  git -C "$repo" config maintenance.auto false
  git -C "$repo" add .
  LEFTHOOK=0 git -C "$repo" commit -m 'fixture base' >/dev/null
  printf '\nfixture second commit\n' >> "$repo/README.md"
  git -C "$repo" add README.md
  LEFTHOOK=0 git -C "$repo" commit -m 'fixture second commit' >/dev/null
}

assert_file_contains() {
  local file="$1"
  local needle="$2"
  if ! grep -Fq -- "$needle" "$file"; then
    printf 'expected %s to contain %s\n' "$file" "$needle" >&2
    exit 1
  fi
}

assert_not_contains() {
  local file="$1"
  local needle="$2"
  if grep -Fq -- "$needle" "$file"; then
    printf 'expected %s to not contain %s\n' "$file" "$needle" >&2
    exit 1
  fi
}

assert_equal() {
  local expected="$1"
  local actual="$2"
  local label="$3"
  if [ "$expected" != "$actual" ]; then
    printf 'expected %s to be %s, got %s\n' "$label" "$expected" "$actual" >&2
    exit 1
  fi
}

assert_exists() {
  local path="$1"
  if [ ! -e "$path" ]; then
    printf 'expected %s to exist\n' "$path" >&2
    exit 1
  fi
}

count_lines() {
  local file="$1"
  if [ -f "$file" ]; then
    wc -l < "$file" | tr -d ' '
  else
    printf '0\n'
  fi
}

write_custom_post_checkout_hook() {
  local repo="$1"
  local hooks_dir
  hooks_dir="$(git -C "$repo" rev-parse --absolute-git-dir)/hooks"
  mkdir -p "$hooks_dir"
  cat > "$hooks_dir/post-checkout" <<'EOF_HOOK'
#!/bin/sh
repo_root="$(git rev-parse --show-toplevel 2>/dev/null || printf '')"
printf '%s\t%s\n' "$repo_root" "$*" >> "${CUSTOM_POST_CHECKOUT_LOG:?}"
EOF_HOOK
  chmod +x "$hooks_dir/post-checkout"
}

write_fake_bun() {
  local bin_dir="$1"
  mkdir -p "$bin_dir"
  cat > "$bin_dir/bun" <<'EOF_FAKE'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\t%s\n' "$(pwd)" "$*" >> "${BUN_INSTALL_LOG:?}"
mkdir -p node_modules
EOF_FAKE
  chmod +x "$bin_dir/bun"
}

write_fake_cargo() {
  local bin_dir="$1"
  mkdir -p "$bin_dir"
  cat > "$bin_dir/cargo" <<'EOF_FAKE'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\t%s\n' "$(pwd)" "$*" >> "${CARGO_FETCH_LOG:?}"
EOF_FAKE
  chmod +x "$bin_dir/cargo"
}

write_fake_lefthook() {
  local bin_dir="$1"
  mkdir -p "$bin_dir"
  cat > "$bin_dir/lefthook" <<'EOF_FAKE'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\t%s\n' "$(pwd)" "$*" >> "${LEFTHOOK_INSTALL_LOG:?}"
if [ "${1:-}" = "install" ]; then
  hooks_dir="$(git rev-parse --git-path hooks)"
  mkdir -p "$hooks_dir"
  cat > "$hooks_dir/pre-commit" <<'EOF_PRE'
#!/bin/sh
printf 'pre-commit\n' >> "${HOOK_CHAIN_LOG:?}"
EOF_PRE
  cat > "$hooks_dir/commit-msg" <<'EOF_COMMIT'
#!/bin/sh
printf 'commit-msg\t%s\n' "${1:-}" >> "${HOOK_CHAIN_LOG:?}"
EOF_COMMIT
  chmod +x "$hooks_dir/pre-commit" "$hooks_dir/commit-msg"
fi
EOF_FAKE
  chmod +x "$bin_dir/lefthook"
}

fixture_repo="$tmp_dir/fixture"
copy_repo "$repo_root" "$fixture_repo"
init_repo "$fixture_repo"

printf 'PRIMARY_SHARED=from-primary\n' > "$fixture_repo/.env"
printf 'PRIMARY_LOCAL=from-primary\n' > "$fixture_repo/.env.local"
printf 'PRIMARY_TEMPLATE=skip-me\n' > "$fixture_repo/.env.example"

fake_bin="$tmp_dir/fake-bin"
bun_log="$tmp_dir/bun.log"
cargo_log="$tmp_dir/cargo.log"
lefthook_log="$tmp_dir/lefthook.log"
custom_hook_log="$tmp_dir/custom-post-checkout.log"
hook_chain_log="$tmp_dir/hook-chain.log"
write_fake_bun "$fake_bin"
write_fake_cargo "$fake_bin"
write_fake_lefthook "$fake_bin"
write_custom_post_checkout_hook "$fixture_repo"

install_output="$(
  cd "$fixture_repo"
  PATH="$fake_bin:$PATH" \
    LEFTHOOK_INSTALL_LOG="$lefthook_log" \
    CUSTOM_POST_CHECKOUT_LOG="$custom_hook_log" \
    HOOK_CHAIN_LOG="$hook_chain_log" \
    bash scripts/install-hooks.sh 2>&1
)"
assert_file_contains <(printf '%s' "$install_output") 'installed shared post-checkout hook'
assert_file_contains "$lefthook_log" $'\t''install'

hooks_dir="$(git -C "$fixture_repo" rev-parse --absolute-git-dir)/hooks"
assert_exists "$hooks_dir/post-checkout"
assert_exists "$hooks_dir/post-checkout.local"
assert_file_contains "$hooks_dir/post-checkout" 'managed by tavily-hikari worktree bootstrap'
assert_exists "$hooks_dir/pre-commit"
assert_exists "$hooks_dir/commit-msg"

worktree_dir="$tmp_dir/linked"
PATH="$fake_bin:$PATH" \
  BUN_INSTALL_LOG="$bun_log" \
  CARGO_FETCH_LOG="$cargo_log" \
  CUSTOM_POST_CHECKOUT_LOG="$custom_hook_log" \
  HOOK_CHAIN_LOG="$hook_chain_log" \
  git -C "$fixture_repo" worktree add --detach "$worktree_dir" HEAD >/dev/null 2>&1

assert_exists "$worktree_dir/.tmp/worktree-bootstrap.v1.done"
assert_file_contains "$worktree_dir/.env" 'PRIMARY_SHARED=from-primary'
assert_file_contains "$worktree_dir/.env.local" 'PRIMARY_LOCAL=from-primary'
if [ -e "$worktree_dir/.env.example" ]; then
  printf 'template env files must not be copied into linked worktrees\n' >&2
  exit 1
fi

assert_exists "$worktree_dir/node_modules"
assert_exists "$worktree_dir/web/node_modules"
assert_exists "$worktree_dir/docs-site/node_modules"
assert_file_contains "$bun_log" "$worktree_dir"$'\t''install --frozen-lockfile'
assert_file_contains "$bun_log" "$worktree_dir/web"$'\t''install --frozen-lockfile'
assert_file_contains "$bun_log" "$worktree_dir/docs-site"$'\t''install --frozen-lockfile'
assert_file_contains "$cargo_log" "$worktree_dir"$'\t''fetch --locked'
assert_file_contains "$custom_hook_log" "$worktree_dir"

bun_before_repeat="$(count_lines "$bun_log")"
cargo_before_repeat="$(count_lines "$cargo_log")"
PATH="$fake_bin:$PATH" \
  BUN_INSTALL_LOG="$bun_log" \
  CARGO_FETCH_LOG="$cargo_log" \
  CUSTOM_POST_CHECKOUT_LOG="$custom_hook_log" \
  HOOK_CHAIN_LOG="$hook_chain_log" \
  git -C "$worktree_dir" checkout --detach HEAD^ >/dev/null 2>&1
PATH="$fake_bin:$PATH" \
  BUN_INSTALL_LOG="$bun_log" \
  CARGO_FETCH_LOG="$cargo_log" \
  CUSTOM_POST_CHECKOUT_LOG="$custom_hook_log" \
  HOOK_CHAIN_LOG="$hook_chain_log" \
  git -C "$worktree_dir" checkout --detach HEAD >/dev/null 2>&1
assert_equal "$bun_before_repeat" "$(count_lines "$bun_log")" 'bun log line count after repeated checkout'
assert_equal "$cargo_before_repeat" "$(count_lines "$cargo_log")" 'cargo log line count after repeated checkout'

printf 'TARGET_LOCAL=keep-me\n' > "$worktree_dir/.env.local"
worktree_setup_output="$(
  cd "$worktree_dir"
  PATH="$fake_bin:$PATH" \
    BUN_INSTALL_LOG="$bun_log" \
    CARGO_FETCH_LOG="$cargo_log" \
    LEFTHOOK_INSTALL_LOG="$lefthook_log" \
    CUSTOM_POST_CHECKOUT_LOG="$custom_hook_log" \
    HOOK_CHAIN_LOG="$hook_chain_log" \
    bash scripts/worktree-setup.sh 2>&1
)"
assert_file_contains <(printf '%s' "$worktree_setup_output") 'linked worktree bootstrap completed'
assert_file_contains "$worktree_dir/.env.local" 'TARGET_LOCAL=keep-me'
assert_not_contains "$worktree_dir/.env.local" 'PRIMARY_LOCAL=from-primary'

primary_bun_before="$(count_lines "$bun_log")"
primary_cargo_before="$(count_lines "$cargo_log")"
primary_output="$(
  cd "$fixture_repo"
  PATH="$fake_bin:$PATH" \
    BUN_INSTALL_LOG="$bun_log" \
    CARGO_FETCH_LOG="$cargo_log" \
    bash scripts/worktree-bootstrap.sh --manual 2>&1
)"
assert_file_contains <(printf '%s' "$primary_output") 'primary worktree detected'
assert_equal "$primary_bun_before" "$(count_lines "$bun_log")" 'bun log line count after primary manual run'
assert_equal "$primary_cargo_before" "$(count_lines "$cargo_log")" 'cargo log line count after primary manual run'

tools_warn_output="$(
  cd "$fixture_repo"
  PATH="/usr/bin:/bin:/usr/sbin:/sbin" \
    bash scripts/install-hooks.sh 2>&1
)"
assert_file_contains <(printf '%s' "$tools_warn_output") 'lefthook not found'

warn_worktree="$tmp_dir/warn-linked"
warn_output="$(
  CUSTOM_POST_CHECKOUT_LOG="$custom_hook_log" \
  PATH="/usr/bin:/bin:/usr/sbin:/sbin" \
    git -C "$fixture_repo" worktree add --detach "$warn_worktree" HEAD 2>&1
)"
assert_file_contains <(printf '%s' "$warn_output") 'bun not found'
assert_file_contains <(printf '%s' "$warn_output") 'cargo not found'
assert_exists "$warn_worktree/.env"
if [ -d "$warn_worktree/node_modules" ] || [ -d "$warn_worktree/web/node_modules" ] || [ -d "$warn_worktree/docs-site/node_modules" ]; then
  printf 'missing-tool bootstrap should not create dependency directories\n' >&2
  exit 1
fi

git -C "$fixture_repo" rm -f scripts/worktree-bootstrap.sh >/dev/null
HOOK_CHAIN_LOG="$hook_chain_log" LEFTHOOK=0 git -C "$fixture_repo" commit -m 'legacy fixture without bootstrap script' >/dev/null
legacy_sha="$(git -C "$fixture_repo" rev-parse HEAD)"
head_sha="$(git -C "$fixture_repo" rev-parse HEAD^)"

PATH="$fake_bin:$PATH" \
  BUN_INSTALL_LOG="$bun_log" \
  CARGO_FETCH_LOG="$cargo_log" \
  CUSTOM_POST_CHECKOUT_LOG="$custom_hook_log" \
  HOOK_CHAIN_LOG="$hook_chain_log" \
  git -C "$worktree_dir" checkout --detach "$legacy_sha" >/dev/null 2>&1
PATH="$fake_bin:$PATH" \
  BUN_INSTALL_LOG="$bun_log" \
  CARGO_FETCH_LOG="$cargo_log" \
  CUSTOM_POST_CHECKOUT_LOG="$custom_hook_log" \
  HOOK_CHAIN_LOG="$hook_chain_log" \
  git -C "$worktree_dir" checkout --detach "$head_sha" >/dev/null 2>&1
assert_file_contains "$worktree_dir/.env.local" 'TARGET_LOCAL=keep-me'

printf 'worktree bootstrap smoke passed\n'
