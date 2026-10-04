#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
WEB_DIR="$ROOT_DIR/web"
RUN_ROOT="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/tavily-hikari-version-layer-reuse-${GITHUB_RUN_ID:-$$}"
DIST_A="$RUN_ROOT/dist-a"
DIST_B="$RUN_ROOT/dist-b"
CONTEXT_A="$RUN_ROOT/context-a"
CONTEXT_A_MTIME="$RUN_ROOT/context-a-mtime"
CONTEXT_B="$RUN_ROOT/context-b"
VERSION_A="${VERSION_A:-0.0.0-ci.1}"
VERSION_B="${VERSION_B:-0.0.0-ci.2}"
IMAGE_PREFIX="${IMAGE_PREFIX:-tavily-hikari-version-layer-${GITHUB_RUN_ID:-$$}}"
REPORT_PATH="${LAYER_ACCEPTANCE_REPORT:-$RUN_ROOT/acceptance.md}"
IMAGE_B_TAG_FILE="${IMAGE_B_TAG_FILE:-}"
SOURCE_DATE_EPOCH=0
INPUT_EPOCH_A=1700000000
INPUT_EPOCH_B=1800000000
PLATFORMS="${PLATFORMS:-linux/amd64}"
CODEX_THREAD_ID="${CODEX_THREAD_ID:-}"
CANDIDATE_SHA="${CANDIDATE_SHA:-$(git -C "$ROOT_DIR" rev-parse HEAD 2>/dev/null || printf 'unbound')}"
INVOCATION_ID=""
BUILDX_BUILDER_ARGS=()
if [[ -n "${BUILDX_BUILDER:-}" ]]; then
  BUILDX_BUILDER_ARGS=(--builder "$BUILDX_BUILDER")
fi

declare -a IMAGE_TAGS=()
declare -a IMAGE_IDS=()
declare -a PLATFORM_RESULTS=()

mkdir -p "$RUN_ROOT"

cleanup() {
  local container_id
  local image_index image_tag expected_image_id current_image_id
  if command -v docker >/dev/null 2>&1; then
    if [[ -n "$INVOCATION_ID" ]]; then
      while IFS= read -r container_id; do
        [[ -n "$container_id" ]] || continue
        docker rm -f "$container_id" >/dev/null 2>&1 || true
      done < <(docker ps -aq --filter "label=codex.testbox.run=$INVOCATION_ID" 2>/dev/null || true)
    fi
    for image_index in "${!IMAGE_TAGS[@]}"; do
      image_tag="${IMAGE_TAGS[$image_index]}"
      expected_image_id="${IMAGE_IDS[$image_index]}"
      current_image_id="$(docker image inspect --format '{{.Id}}' "$image_tag" 2>/dev/null || true)"
      if [[ "$current_image_id" == "$expected_image_id" ]]; then
        docker image rm "$image_tag" >/dev/null 2>&1 || true
      fi
    done
  fi
  if [[ "${KEEP_RUN_ROOT:-0}" != "1" ]]; then
    rm -rf "$RUN_ROOT"
  fi
}
trap cleanup EXIT

command -v docker >/dev/null
command -v bun >/dev/null
command -v python3 >/dev/null
docker buildx version >/dev/null
[[ "$VERSION_A" != "$VERSION_B" ]]

if [[ -n "$CODEX_THREAD_ID" && ! "$CODEX_THREAD_ID" =~ ^[a-z0-9][a-z0-9_-]*$ ]]; then
  echo "CODEX_THREAD_ID must match ^[a-z0-9][a-z0-9_-]*$ for labeled testbox containers" >&2
  exit 2
fi

INVOCATION_ID="$(python3 -c 'import uuid; print(uuid.uuid4())')"
IMAGE_TAG_PREFIX="${IMAGE_PREFIX}-${INVOCATION_ID}"
if [[ -n "$CODEX_THREAD_ID" ]]; then
  CONTAINER_PREFIX="testbox-${CODEX_THREAD_ID}-${IMAGE_PREFIX}-${INVOCATION_ID}"
else
  CONTAINER_PREFIX="${IMAGE_PREFIX}-${INVOCATION_ID}"
fi
DOCKER_LABEL_ARGS=(--label "codex.testbox.run=$INVOCATION_ID")
if [[ -n "$CODEX_THREAD_ID" ]]; then
  DOCKER_LABEL_ARGS+=(--label "codex.testbox.agent=$CODEX_THREAD_ID")
fi

IFS=, read -r -a PLATFORM_LIST <<< "$PLATFORMS"
if ((${#PLATFORM_LIST[@]} == 0)); then
  echo "PLATFORMS must contain at least one comma-separated Docker platform" >&2
  exit 2
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
    VITE_APP_VERSION="$version" WEB_DIST_DIR="$output_dir" python3 ./scripts/generate_pwa_assets.py
  )

  if [[ -e "$output_dir/version.json" ]]; then
    echo "production web output must not contain static version.json" >&2
    exit 1
  fi
  if grep -Eiq '<meta[^>]+(app-)?version' "$output_dir"/*.html; then
    echo "production HTML must not contain a version meta tag" >&2
    exit 1
  fi
  for worker in sw-public.js sw-admin.js; do
    grep -Fq "const BUILD_VERSION = \"$version\";" "$output_dir/$worker"
  done
  if ! grep -rFl --include='*.js' -e "\"$version\"" "$output_dir/assets" >/dev/null; then
    echo "frontend JavaScript bundle does not embed $version" >&2
    exit 1
  fi
}

build_web "$VERSION_A" "$DIST_A"
build_web "$VERSION_B" "$DIST_B"

python3 - "$DIST_A" "$DIST_B" <<'PY'
import pathlib
import sys

left, right = map(pathlib.Path, sys.argv[1:])
for relative in ("favicon.svg", "manifest.webmanifest", "manifest-admin.webmanifest"):
    if (left / relative).read_bytes() != (right / relative).read_bytes():
        raise SystemExit(f"stable web file changed between SemVer builds: {relative}")

left_icons = sorted(path.name for path in (left / "pwa").glob("*.png"))
right_icons = sorted(path.name for path in (right / "pwa").glob("*.png"))
if left_icons != right_icons:
    raise SystemExit("PWA icon file names changed between SemVer builds")
for name in left_icons:
    if (left / "pwa" / name).read_bytes() != (right / "pwa" / name).read_bytes():
        raise SystemExit(f"stable PWA icon changed between SemVer builds: {name}")

print(f"Verified {len(left_icons)} stable PWA icons and stable manifests/favicon")
PY

prepare_context() {
  local output="$1"
  local dist="$2"
  local epoch="$3"

  rm -rf "$output"
  mkdir -p "$output/scripts" "$output/web"
  cp "$ROOT_DIR/Dockerfile" "$ROOT_DIR/.dockerignore" "$ROOT_DIR/Cargo.toml" \
    "$ROOT_DIR/Cargo.lock" "$ROOT_DIR/build.rs" "$ROOT_DIR/rust-toolchain.toml" "$output/"
  cp -a "$ROOT_DIR/src" "$output/src"
  cp "$ROOT_DIR/scripts/docker-entrypoint.sh" "$ROOT_DIR/scripts/docker-healthcheck.sh" "$output/scripts/"
  cp -a "$dist" "$output/web/dist"
  find "$output" -exec touch -h -d "@$epoch" {} +
}

prepare_context "$CONTEXT_A" "$DIST_A" "$INPUT_EPOCH_A"
prepare_context "$CONTEXT_A_MTIME" "$DIST_A" "$INPUT_EPOCH_B"
prepare_context "$CONTEXT_B" "$DIST_B" "$INPUT_EPOCH_A"

hash_layers() {
  docker image inspect --format '{{range .RootFS.Layers}}{{println .}}{{end}}' "$1"
}

buildx_build() {
  docker buildx build "${BUILDX_BUILDER_ARGS[@]}" "$@"
}

track_image() {
  local image_tag="$1"
  local image_id
  image_id="$(docker image inspect --format '{{.Id}}' "$image_tag")"
  IMAGE_TAGS+=("$image_tag")
  IMAGE_IDS+=("$image_id")
}

build_image() {
  local platform="$1"
  local tag="$2"
  local version="$3"
  local context="$4"
  local force_normalizer="${5:-false}"
  local -a build_args=(
    --platform "$platform"
    --load
    --tag "$tag"
    --build-arg "APP_EFFECTIVE_VERSION=$version"
    --build-arg "SOURCE_DATE_EPOCH=$SOURCE_DATE_EPOCH"
  )
  if [[ "$force_normalizer" == "true" ]]; then
    build_args+=(--no-cache-filter=payload-normalizer)
  fi
  buildx_build "${build_args[@]}" "$context"
  track_image "$tag"
}

check_image_contract() {
  local image="$1"
  local expected_version="$2"
  local container_name="$3"
  local port response cli_version

  cli_version="$(docker run --rm \
    --name "${container_name}-version" \
    "${DOCKER_LABEL_ARGS[@]}" \
    --entrypoint /usr/local/bin/tavily-hikari \
    "$image" --version)"
  if [[ "$cli_version" != "tavily-hikari $expected_version" ]]; then
    echo "$image reports unexpected CLI version: $cli_version" >&2
    return 1
  fi
  printf -- '- Image `%s`: `--version` reports `%s`.\n' "$expected_version" "$cli_version" \
    >> "$RUN_ROOT/cli-version-results.md"

  docker run --detach \
    --name "$container_name" \
    "${DOCKER_LABEL_ARGS[@]}" \
    --publish 127.0.0.1::8787 \
    --env PROXY_BIND=0.0.0.0 \
    --env TAVILY_API_KEYS=tvly-ci-layer-check \
    --env TAVILY_UPSTREAM=http://127.0.0.1:9/mcp \
    --env DEV_OPEN_ADMIN=true \
    "$image" >/dev/null
  port="$(docker port "$container_name" 8787/tcp | sed -n 's/.*://p' | head -n 1)"

  local api_ok=false
  for _ in $(seq 1 60); do
    if response="$(curl -fsS "http://127.0.0.1:${port}/api/version" 2>/dev/null)" \
      && EXPECTED_VERSION="$expected_version" VERSION_RESPONSE="$response" python3 - <<'PY'
import json
import os
payload = json.loads(os.environ["VERSION_RESPONSE"])
expected = os.environ["EXPECTED_VERSION"]
if payload.get("backend") != expected or payload.get("frontend") != expected:
    raise SystemExit(1)
PY
    then
      api_ok=true
      break
    fi
    sleep 1
  done
  if [[ "$api_ok" != true ]]; then
    docker logs "$container_name" >&2 || true
    echo "$image did not report backend/frontend version $expected_version" >&2
    return 1
  fi

  response="$(curl -fsS "http://127.0.0.1:${port}/version.json")"
  EXPECTED_VERSION="$expected_version" VERSION_RESPONSE="$response" python3 - <<'PY'
import json
import os
payload = json.loads(os.environ["VERSION_RESPONSE"])
if payload != {"version": os.environ["EXPECTED_VERSION"]}:
    raise SystemExit(f"unexpected /version.json response: {payload!r}")
PY

  docker exec "$container_name" test ! -e /srv/app/web/version.json
  docker exec "$container_name" grep -a -Fq "$expected_version" /usr/local/bin/tavily-hikari
  EXPECTED_VERSION="$expected_version" IMAGE_INSPECT="$(docker image inspect "$image")" python3 - <<'PY'
import json
import os
image = json.loads(os.environ["IMAGE_INSPECT"])[0]
version = os.environ["EXPECTED_VERSION"]
label = image.get("Config", {}).get("Labels", {}).get("org.opencontainers.image.version")
if label != version:
    raise SystemExit(f"OCI version label {label!r} does not match {version!r}")
if any(value.startswith("APP_EFFECTIVE_VERSION=") for value in image.get("Config", {}).get("Env", [])):
    raise SystemExit("APP_EFFECTIVE_VERSION leaked into runtime Config.Env")
PY
}

analyze_changed_layers() {
  local image_a="$1"
  local image_b="$2"
  local report_file="$3"
  local platform="$4"

  docker image save --output "$RUN_ROOT/layer-a.tar" "$image_a"
  docker image save --output "$RUN_ROOT/layer-b.tar" "$image_b"
  python3 - "$RUN_ROOT/layer-a.tar" "$RUN_ROOT/layer-b.tar" "$report_file" "$platform" <<'PY'
import gzip
import io
import json
import pathlib
import sys
import tarfile

archive_a, archive_b, report_path = map(pathlib.Path, sys.argv[1:4])
platform = sys.argv[4]

def load_image(path):
    with tarfile.open(path, "r") as archive:
        manifest = json.load(archive.extractfile("manifest.json"))[0]
        config = json.load(archive.extractfile(manifest["Config"]))
        layers = []
        names = []
        for layer_path in manifest["Layers"]:
            data = archive.extractfile(layer_path).read()
            layers.append(data)
            with tarfile.open(fileobj=io.BytesIO(data), mode="r:*") as layer:
                names.append([member.name.lstrip("./") for member in layer.getmembers()])
        return config["rootfs"]["diff_ids"], layers, names

diff_a, blobs_a, names_a = load_image(archive_a)
diff_b, blobs_b, names_b = load_image(archive_b)
if len(diff_a) != len(diff_b):
    raise SystemExit("SemVer A/B images have different filesystem layer counts")
changed = [index for index, pair in enumerate(zip(diff_a, diff_b)) if pair[0] != pair[1]]
if len(changed) != 2:
    print(f"expected exactly two SemVer-dependent filesystem layers, found {changed}", file=sys.stderr)
    for index in changed:
        paths_a = set(names_a[index])
        paths_b = set(names_b[index])
        only_a = sorted(paths_a - paths_b)
        only_b = sorted(paths_b - paths_a)
        print(f"layer {index}: paths only in A ({len(only_a)}): {only_a[:5]}", file=sys.stderr)
        print(f"layer {index}: paths only in B ({len(only_b)}): {only_b[:5]}", file=sys.stderr)
        if paths_a == paths_b:
            print(f"layer {index}: same {len(paths_a)} paths have different content or metadata", file=sys.stderr)
    raise SystemExit(1)

entries = []
for index in changed:
    paths = names_b[index]
    if any(path == "usr/local/bin/tavily-hikari" for path in paths):
        layer_type = "main-service-binary"
    elif any(path.startswith("srv/app/web/") for path in paths):
        layer_type = "frontend-application"
        if not any(path.startswith("srv/app/web/assets/") and path.endswith(".js") for path in paths):
            raise SystemExit("frontend application layer changed without a real JavaScript bundle")
    else:
        raise SystemExit(f"changed layer {index} is not the server binary or frontend application: {paths}")
    compressed_size = len(gzip.compress(blobs_b[index], compresslevel=1, mtime=0))
    entries.append((index, layer_type, compressed_size, paths))

if {entry[1] for entry in entries} != {"main-service-binary", "frontend-application"}:
    raise SystemExit(f"unexpected changed layer types: {[entry[1] for entry in entries]}")
if any("srv/app/web/version.json" in paths for paths in names_b):
    raise SystemExit("static version.json exists in the production image")

with report_path.open("a", encoding="utf-8") as report:
    for index, layer_type, size, paths in entries:
        report.write(f"| {platform} | {index} | {layer_type} | {size} | gzip -1 of docker-save layer.tar |\n")
print("SemVer-changing layers:")
for index, layer_type, size, _ in entries:
    print(f"  index={index} type={layer_type} compressed_bytes={size}")
PY
}

REPORT_DIR="$(dirname -- "$REPORT_PATH")"
mkdir -p "$REPORT_DIR"
CHANGED_LAYER_REPORT="$RUN_ROOT/changed-layers.md"
: > "$CHANGED_LAYER_REPORT"
cat > "$REPORT_PATH" <<EOF
# OCI version/layer reuse acceptance

- Candidate: $CANDIDATE_SHA
- Source date epoch: $SOURCE_DATE_EPOCH
- Synthetic package versions: $VERSION_A and $VERSION_B
- Platforms: $PLATFORMS
- Scenario: same source and SemVer with different Docker-context mtimes; then same source with different SemVer
- Note: SemVer A/B is a synthetic packaging contract test, not evidence of a historical production version-only release.

## Same-version mtime comparison

| Platform | Filesystem layers | Same-version mtime comparison | SemVer layer changes |
| --- | ---: | --- | --- |
EOF

first_platform="${PLATFORM_LIST[0]}"
audit_tag="${IMAGE_TAG_PREFIX}-context-audit"
buildx_build --platform "$first_platform" --load --target context-audit --tag "$audit_tag" "$CONTEXT_A"
track_image "$audit_tag"

for platform in "${PLATFORM_LIST[@]}"; do
  arch="${platform##*/}"
  image_a="${IMAGE_TAG_PREFIX}-a-${arch}"
  image_a_mtime="${IMAGE_TAG_PREFIX}-a-mtime-${arch}"
  image_b="${IMAGE_TAG_PREFIX}-b-${arch}"
  if [[ "$arch" == "amd64" ]]; then
    image_b="${IMAGE_TAG_PREFIX}-b"
  fi

  build_image "$platform" "$image_a" "$VERSION_A" "$CONTEXT_A"
  build_image "$platform" "$image_a_mtime" "$VERSION_A" "$CONTEXT_A_MTIME" true
  build_image "$platform" "$image_b" "$VERSION_B" "$CONTEXT_B"

  layers_a="$(hash_layers "$image_a")"
  layers_a_mtime="$(hash_layers "$image_a_mtime")"
  layers_b="$(hash_layers "$image_b")"
  if [[ "$layers_a" != "$layers_a_mtime" ]]; then
    diff -u <(printf '%s\n' "$layers_a") <(printf '%s\n' "$layers_a_mtime") >&2 || true
    echo "$platform same-version image layers differ when only input mtimes change" >&2
    exit 1
  fi
  printf '%s\n' "$layers_a" > "$RUN_ROOT/${arch}-rootfs-diffids.txt"

  layer_count="$(printf '%s\n' "$layers_a" | sed '/^$/d' | wc -l | tr -d ' ')"
  check_image_contract "$image_a" "$VERSION_A" "${CONTAINER_PREFIX}-${arch}-a"
  check_image_contract "$image_b" "$VERSION_B" "${CONTAINER_PREFIX}-${arch}-b"
  printf '| %s | %s | all RootFS diffIDs match | exactly main binary + frontend application |\n' \
    "$platform" "$layer_count" >> "$REPORT_PATH"
  analyze_changed_layers "$image_a" "$image_b" "$CHANGED_LAYER_REPORT" "$platform"
  PLATFORM_RESULTS+=("$platform: $layer_count layers, mtime normalized, exactly two SemVer-changing layers")

  if [[ -n "${IMAGE_B_ARCHIVE:-}" && "$arch" == "amd64" ]]; then
    mkdir -p "$(dirname -- "$IMAGE_B_ARCHIVE")"
    docker save "$image_b" | gzip -1 > "$IMAGE_B_ARCHIVE"
    if [[ -n "$IMAGE_B_TAG_FILE" ]]; then
      mkdir -p "$(dirname -- "$IMAGE_B_TAG_FILE")"
      printf '%s\n' "$image_b" > "$IMAGE_B_TAG_FILE"
    fi
  fi
done

{
  printf '\n## Same-version RootFS diffIDs\n\n'
  printf '| Platform | Index | RootFS diffID |\n'
  printf '| --- | ---: | --- |\n'
  for platform in "${PLATFORM_LIST[@]}"; do
    arch="${platform##*/}"
    index=0
    while IFS= read -r diff_id; do
      [[ -n "$diff_id" ]] || continue
      printf '| %s | %s | `%s` |\n' "$platform" "$index" "$diff_id"
      index=$((index + 1))
    done < "$RUN_ROOT/${arch}-rootfs-diffids.txt"
  done
  printf '\n## Changed layer compressed byte estimates\n\n'
  printf '| Platform | RootFS diffID index | Layer payload | Compressed bytes | Method |\n'
  printf '| --- | ---: | --- | ---: | --- |\n'
  cat "$CHANGED_LAYER_REPORT"
  printf '\n## Packaged CLI versions\n\n'
  cat "$RUN_ROOT/cli-version-results.md"
} >> "$REPORT_PATH"

echo "OCI version/layer reuse acceptance passed: ${PLATFORM_RESULTS[*]}"
echo "Acceptance report: $REPORT_PATH"
