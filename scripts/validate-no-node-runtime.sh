#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_DIR="$(mktemp -d "$ROOT_DIR/.tmp-no-node-runtime.XXXXXX")"
SHIM_DIR="$TMP_DIR/shim"
HOOK_FIXTURES_DIR="$TMP_DIR/hook-fixtures"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

mkdir -p "$SHIM_DIR" "$HOOK_FIXTURES_DIR"

cat >"$SHIM_DIR/node" <<'SHIM'
#!/usr/bin/env bash
echo "unexpected node invocation: $*" >&2
exit 1
SHIM
chmod +x "$SHIM_DIR/node"

cat >"$HOOK_FIXTURES_DIR/sample.md" <<'MARKDOWN'
# no-node smoke

This file ensures dprint walks the real hook command path.
MARKDOWN

cat >"$HOOK_FIXTURES_DIR/COMMIT_EDITMSG" <<'COMMIT'
chore: validate bun runtime

Exercise commitlint without allowing a node fallback.
COMMIT

export PATH="$SHIM_DIR:$PATH"

pushd "$ROOT_DIR" >/dev/null

echo "[no-node] using shim at $(command -v node)"
echo "[no-node] exercising repo hook command paths"
bunx --bun dprint fmt "$HOOK_FIXTURES_DIR/sample.md"
bunx --bun commitlint --edit "$HOOK_FIXTURES_DIR/COMMIT_EDITMSG"
(
  cd web
  bun run build
  bun run build-storybook
)

popd >/dev/null
