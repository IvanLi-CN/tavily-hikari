#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"

bash "$repo_root/scripts/install-hooks.sh"
bash "$repo_root/scripts/worktree-bootstrap.sh" --manual --force --strict "$@"
