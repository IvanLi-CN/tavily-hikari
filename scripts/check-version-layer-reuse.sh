#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
WEB_DIR="$ROOT_DIR/web"
RUN_ROOT="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/tavily-hikari-version-layer-reuse-${GITHUB_RUN_ID:-$$}"
DIST_A="$RUN_ROOT/dist-a"
DIST_B="$RUN_ROOT/dist-b"
ORIGINAL_DIST="$RUN_ROOT/original-dist"
VERSION_A="${VERSION_A:-ci-layer-a}"
VERSION_B="${VERSION_B:-ci-layer-b}"
IMAGE_PREFIX="${IMAGE_PREFIX:-tavily-hikari-version-layer-${GITHUB_RUN_ID:-$$}}"
IMAGE_A="${IMAGE_PREFIX}-a"
IMAGE_B="${IMAGE_PREFIX}-b"
AUDIT_IMAGE="${IMAGE_PREFIX}-context-audit"
CONTAINER_A="${IMAGE_PREFIX}-container-a"
CONTAINER_B="${IMAGE_PREFIX}-container-b"

mkdir -p "$RUN_ROOT"

restore_dist() {
  rm -rf "$WEB_DIR/dist"
  if [[ -d "$ORIGINAL_DIST" ]]; then
    mv "$ORIGINAL_DIST" "$WEB_DIR/dist"
  fi
}

cleanup() {
  restore_dist || true
  if command -v docker >/dev/null 2>&1; then
    docker rm -fv "$CONTAINER_A" "$CONTAINER_B" >/dev/null 2>&1 || true
    docker image rm "$IMAGE_A" "$IMAGE_B" "$AUDIT_IMAGE" >/dev/null 2>&1 || true
  fi
  rm -rf "$RUN_ROOT"
}

trap cleanup EXIT

if [[ -d "$WEB_DIR/dist" ]]; then
  cp -a "$WEB_DIR/dist" "$ORIGINAL_DIST"
fi

(
  cd "$WEB_DIR"
  bun --bun ./node_modules/.bin/tsc -b
)

build_web() {
  local version="$1"
  local output_dir="$2"

  rm -rf "$output_dir"
  mkdir -p "$output_dir"
  (
    cd "$WEB_DIR"
    VITE_APP_VERSION="$version" bun --bun ./node_modules/.bin/vite build --outDir "$output_dir"
    VITE_APP_VERSION="$version" WEB_DIST_DIR="$output_dir" bun ./scripts/write-version.mjs
    WEB_DIST_DIR="$output_dir" python3 ./scripts/generate_pwa_assets.py
  )
  # Normalize stable files so their COPY layers are reproducible, while leaving
  # version-bearing files with build-specific metadata to prevent cache aliasing.
  find "$output_dir" -type d -exec touch -t 197001010000 {} +
  while IFS= read -r file; do
    case "$file" in
      "$output_dir/version.json"|"$output_dir/index.html"|"$output_dir/admin.html"|"$output_dir/console.html"|"$output_dir/login.html"|"$output_dir/registration-paused.html"|"$output_dir/sw-public.js"|"$output_dir/sw-admin.js")
        continue
        ;;
    esac
    touch -t 197001010000 "$file"
  done < <(find "$output_dir" -type f -print)
}

hash_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d ' ' -f 1
  else
    shasum -a 256 "$1" | cut -d ' ' -f 1
  fi
}

build_web "$VERSION_A" "$DIST_A"
build_web "$VERSION_B" "$DIST_B"

mapfile -t files_a < <(cd "$DIST_A" && find . -type f -print | sort)
mapfile -t files_b < <(cd "$DIST_B" && find . -type f -print | sort)
if ! diff -u <(printf '%s\n' "${files_a[@]}") <(printf '%s\n' "${files_b[@]}"); then
  echo "version A/B output file inventories differ" >&2
  exit 1
fi

dynamic_files=(
  "./version.json"
  "./index.html"
  "./admin.html"
  "./console.html"
  "./login.html"
  "./registration-paused.html"
  "./sw-public.js"
  "./sw-admin.js"
)
for file in "${dynamic_files[@]}"; do
  if cmp -s "$DIST_A/$file" "$DIST_B/$file"; then
    echo "expected version-specific output to differ: $file" >&2
    exit 1
  fi
done

for file in "${files_a[@]}"; do
  if printf '%s\n' "${dynamic_files[@]}" | grep -Fxq "$file"; then
    continue
  fi
  hash_a="$(hash_file "$DIST_A/$file")"
  hash_b="$(hash_file "$DIST_B/$file")"
  if [[ "$hash_a" != "$hash_b" ]]; then
    echo "stable output changed between versions: $file" >&2
    exit 1
  fi
done

grep -Fq "\"version\": \"$VERSION_A\"" "$DIST_A/version.json"
grep -Fq "\"version\": \"$VERSION_B\"" "$DIST_B/version.json"
grep -R -Fq "const BUILD_VERSION = \"$VERSION_A\";" "$DIST_A/sw-public.js" "$DIST_A/sw-admin.js"
grep -R -Fq "const BUILD_VERSION = \"$VERSION_B\";" "$DIST_B/sw-public.js" "$DIST_B/sw-admin.js"

rm -rf "$WEB_DIR/dist"
cp -a "$DIST_A" "$WEB_DIR/dist"
docker build --target context-audit --build-arg "APP_EFFECTIVE_VERSION=$VERSION_A" -t "$AUDIT_IMAGE" "$ROOT_DIR"
docker build --build-arg "APP_EFFECTIVE_VERSION=$VERSION_A" -t "$IMAGE_A" "$ROOT_DIR"

rm -rf "$WEB_DIR/dist"
cp -a "$DIST_B" "$WEB_DIR/dist"
docker build --build-arg "APP_EFFECTIVE_VERSION=$VERSION_B" -t "$IMAGE_B" "$ROOT_DIR"

mapfile -t layers_a < <(docker image inspect --format '{{range .RootFS.Layers}}{{println .}}{{end}}' "$IMAGE_A")
mapfile -t layers_b < <(docker image inspect --format '{{range .RootFS.Layers}}{{println .}}{{end}}' "$IMAGE_B")
if [[ "${#layers_a[@]}" -lt 2 || "${#layers_a[@]}" -ne "${#layers_b[@]}" ]]; then
  echo "version A/B image layer counts differ or are empty" >&2
  exit 1
fi
dynamic_layer_index=$((${#layers_a[@]} - 2))
for ((index = 0; index < dynamic_layer_index; index += 1)); do
  if [[ "${layers_a[$index]}" != "${layers_b[$index]}" ]]; then
    echo "stable image layer changed at index $index" >&2
    exit 1
  fi
done
# VOLUME may leave an empty trailing RootFS layer after the dynamic COPY.
if [[ "${layers_a[$dynamic_layer_index]}" == "${layers_b[$dynamic_layer_index]}" ]]; then
  echo "version-specific final image layer did not change" >&2
  exit 1
fi
if [[ "${layers_a[$((${#layers_a[@]} - 1))]}" != "${layers_b[$((${#layers_b[@]} - 1))]}" ]]; then
  echo "post-metadata image layer changed unexpectedly" >&2
  exit 1
fi

check_api_version() {
  local image_name="$1"
  local container_name="$2"
  local expected_version="$3"
  local port
  local response

  docker run --detach \
    --name "$container_name" \
    --publish 127.0.0.1::8787 \
    --env PROXY_BIND=0.0.0.0 \
    --env TAVILY_API_KEYS=tvly-ci-layer-check \
    --env TAVILY_UPSTREAM=http://127.0.0.1:9/mcp \
    --env DEV_OPEN_ADMIN=true \
    "$image_name" >/dev/null
  port="$(docker port "$container_name" 8787/tcp | sed -n 's/.*://p' | head -n 1)"
  for _ in $(seq 1 60); do
    response="$(curl -fsS "http://127.0.0.1:${port}/api/version" 2>/dev/null || true)"
    if EXPECTED_VERSION="$expected_version" VERSION_RESPONSE="$response" python3 - <<'PY'
import json
import os

payload = json.loads(os.environ["VERSION_RESPONSE"])
expected = os.environ["EXPECTED_VERSION"]
if payload.get("backend") != expected or payload.get("frontend") != expected:
    raise SystemExit(1)
PY
    then
      return 0
    fi
    sleep 1
  done
  docker logs "$container_name" >&2 || true
  echo "${image_name} did not report release version ${expected_version}" >&2
  return 1
}

check_api_version "$IMAGE_A" "$CONTAINER_A" "$VERSION_A"
check_api_version "$IMAGE_B" "$CONTAINER_B" "$VERSION_B"

for image_version in "$VERSION_A" "$VERSION_B"; do
  image_name="$IMAGE_A"
  [[ "$image_version" == "$VERSION_B" ]] && image_name="$IMAGE_B"
  docker run --rm --entrypoint /bin/sh "$image_name" -c "grep -Fq '\"version\": \"$image_version\"' /srv/app/web/version.json"
  env_version="$(docker image inspect --format '{{range .Config.Env}}{{println .}}{{end}}' "$image_name" | sed -n "s/^APP_EFFECTIVE_VERSION=//p")"
  label_version="$(docker image inspect --format '{{index .Config.Labels "org.opencontainers.image.version"}}' "$image_name")"
  [[ "$env_version" == "$image_version" ]]
  [[ "$label_version" == "$image_version" ]]
done

if [[ -n "${IMAGE_B_ARCHIVE:-}" ]]; then
  mkdir -p "$(dirname -- "$IMAGE_B_ARCHIVE")"
  docker save "$IMAGE_B" | gzip -1 > "$IMAGE_B_ARCHIVE"
fi

echo "version layer reuse check passed for $VERSION_A and $VERSION_B"
