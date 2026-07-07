#!/usr/bin/env bash
set -euo pipefail

REPO_URL="https://github.com/IvanLi-CN/tavily-hikari"
ASSET_BASE_URL="${TAVILY_HIKARI_RELEASE_BASE_URL:-${REPO_URL}/releases/latest/download}"
MIN_TVLY_VERSION="0.1.3"
INSTALL_DIR="${HOME}/.local/bin"
CONFIG_DIR="${XDG_CONFIG_HOME:-${HOME}/.config}/tavily-hikari-cli"
BASE_URL=""
TOKEN=""
WITH_SKILLS=0

usage() {
  cat <<'EOF'
Install tvly-hikari, a thin Tavily Hikari wrapper for the official tvly CLI.

Usage:
  install-tvly-hikari.sh [options]

Options:
  --base-url <origin>    Hikari origin, for example https://hikari.example.com
  --token <th-...>       Hikari access token to store in the local config
  --install-dir <dir>    CLI install directory (default: ~/.local/bin)
  --config-dir <dir>     Config directory (default: ~/.config/tavily-hikari-cli)
  --with-skills          Also run: npx skills add https://github.com/IvanLi-CN/tavily-hikari
  -h, --help             Show this help

The installer writes config.json with mode 0600 when --base-url and --token are provided.
EOF
}

die() {
  printf 'install-tvly-hikari: %s\n' "$*" >&2
  exit 1
}

tvly_version() {
  local tvly_bin="$1"
  "${tvly_bin}" --version 2>/dev/null \
    | python3 -c 'import re, sys; data=sys.stdin.read(); match=re.search(r"\d+\.\d+\.\d+", data); print(match.group(0) if match else "")'
}

tvly_is_compatible() {
  local tvly_bin="$1"
  command -v python3 >/dev/null 2>&1 || return 1
  local version
  version="$(tvly_version "${tvly_bin}")"
  [[ -n "${version}" ]] || return 1
  python3 - "${version}" "${MIN_TVLY_VERSION}" <<'PY'
import sys

def parse(value):
    return tuple(int(part) for part in value.split("."))

installed, minimum = sys.argv[1:3]
raise SystemExit(0 if parse(installed) >= parse(minimum) else 1)
PY
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --base-url)
      [[ $# -ge 2 ]] || die "--base-url requires a value"
      BASE_URL="$2"
      shift 2
      ;;
    --token)
      [[ $# -ge 2 ]] || die "--token requires a value"
      TOKEN="$2"
      shift 2
      ;;
    --install-dir)
      [[ $# -ge 2 ]] || die "--install-dir requires a value"
      INSTALL_DIR="$2"
      shift 2
      ;;
    --config-dir)
      [[ $# -ge 2 ]] || die "--config-dir requires a value"
      CONFIG_DIR="$2"
      shift 2
      ;;
    --with-skills)
      WITH_SKILLS=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown option: $1"
      ;;
  esac
done

if [[ -n "${BASE_URL}" || -n "${TOKEN}" ]]; then
  [[ -n "${BASE_URL}" ]] || die "--base-url is required when --token is provided"
  [[ -n "${TOKEN}" ]] || die "--token is required when --base-url is provided"
fi

ensure_tvly() {
  if command -v tvly >/dev/null 2>&1; then
    if tvly_is_compatible "$(command -v tvly)"; then
      return
    fi
    printf 'official tvly CLI is missing custom base URL support; upgrading tavily-cli with uv...\n'
  fi

  if command -v uv >/dev/null 2>&1; then
    if ! command -v tvly >/dev/null 2>&1; then
      printf 'official tvly CLI not found; installing tavily-cli with uv...\n'
    fi
    uv tool install --upgrade tavily-cli
    export PATH="${HOME}/.local/bin:${PATH}"
  fi

  if ! command -v tvly >/dev/null 2>&1; then
    cat >&2 <<'EOF'
official tvly CLI is required but was not found.

Install it manually, then re-run this installer:
  uv tool install --upgrade tavily-cli

If uv is not installed, see:
  https://docs.astral.sh/uv/getting-started/installation/
EOF
    exit 1
  fi

  if ! tvly_is_compatible "$(command -v tvly)"; then
    cat >&2 <<EOF
official tvly CLI must be tavily-cli >= ${MIN_TVLY_VERSION} so Hikari can override TAVILY_API_BASE_URL.

Upgrade it manually, then re-run this installer:
  uv tool install --upgrade tavily-cli
EOF
    exit 1
  fi
}

install_wrapper() {
  install -d -m 0755 "${INSTALL_DIR}"
  local target="${INSTALL_DIR}/tvly-hikari"
  local real_target="${INSTALL_DIR}/.tvly-hikari-real"

  if [[ -n "${HIKARI_INSTALL_LOCAL_TVLY_HIKARI:-}" ]]; then
    [[ -f "${HIKARI_INSTALL_LOCAL_TVLY_HIKARI}" ]] || die "local wrapper not found: ${HIKARI_INSTALL_LOCAL_TVLY_HIKARI}"
    install -m 0755 "${HIKARI_INSTALL_LOCAL_TVLY_HIKARI}" "${real_target}"
  elif command -v curl >/dev/null 2>&1; then
    curl -fsSL "${ASSET_BASE_URL}/tvly-hikari" -o "${real_target}"
    chmod 0755 "${real_target}"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "${real_target}" "${ASSET_BASE_URL}/tvly-hikari"
    chmod 0755 "${real_target}"
  else
    die "curl or wget is required to download ${ASSET_BASE_URL}/tvly-hikari"
  fi

  python3 - "${target}" "${real_target}" "${CONFIG_DIR}" <<'PY'
import shlex
import sys
from pathlib import Path

target, real_target, config_dir = sys.argv[1:4]
Path(target).write_text(
    "#!/usr/bin/env bash\n"
    "set -euo pipefail\n"
    'if [[ -z "${TAVILY_HIKARI_CONFIG_DIR:-}" && -z "${TAVILY_HIKARI_CONFIG_FILE:-}" ]]; then\n'
    f"  export TAVILY_HIKARI_CONFIG_DIR={shlex.quote(config_dir)}\n"
    "fi\n"
    f"exec {shlex.quote(real_target)} \"$@\"\n",
    encoding="utf-8",
)
Path(target).chmod(0o755)
PY

  printf 'installed %s\n' "${target}"
}

configure_wrapper() {
  if [[ -z "${BASE_URL}" && -z "${TOKEN}" ]]; then
    cat <<EOF
Run this next to configure a Hikari endpoint:
  ${INSTALL_DIR}/tvly-hikari configure --base-url <Hikari origin> --token <th-...>
EOF
    return
  fi

  "${INSTALL_DIR}/tvly-hikari" configure \
    --base-url "${BASE_URL}" \
    --token "${TOKEN}" \
    --config-dir "${CONFIG_DIR}"
}

install_skills() {
  [[ "${WITH_SKILLS}" -eq 1 ]] || return 0
  command -v npx >/dev/null 2>&1 || die "--with-skills requires npx"
  npx skills add "${REPO_URL}"
}

ensure_tvly
install_wrapper
configure_wrapper
install_skills

cat <<EOF
Done.

Try:
  ${INSTALL_DIR}/tvly-hikari search "latest AI agent news" --json

Optional Agent Skills install:
  npx skills add ${REPO_URL}
EOF
