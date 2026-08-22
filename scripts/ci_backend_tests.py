#!/usr/bin/env python3

import argparse
import concurrent.futures
import hashlib
import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import time
from collections import defaultdict
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
MANIFEST_PATH = ROOT / "scripts" / "ci_backend_test_manifest.json"
BASE_CARGO_ARGS = ["cargo", "test", "--locked", "--all-features"]
CARGO_LIST_TIMEOUT_SECONDS = 300
EXECUTABLE_LIST_TIMEOUT_SECONDS = 30
EXECUTABLE_LIST_RETRY_TIMEOUT_SECONDS = 60
DEFAULT_BENCHMARK_WORKERS = max(1, os.cpu_count() or 1)
DEFAULT_LOW_RESOURCE_CARGO_JOBS = 2
DEFAULT_LOW_RESOURCE_FILTERED_PROCESS_WORKERS = 1
DEFAULT_LOW_RESOURCE_FILTERED_TEST_THREADS = 2
DIAGNOSTIC_CARGO_JOBS = 1
DIAGNOSTIC_FILTERED_PROCESS_WORKERS = 1
DIAGNOSTIC_FILTERED_TEST_THREADS = 1
ARTIFACT_MANIFEST_NAME = "artifact-manifest.json"
ARTIFACT_FORMAT_VERSION = 2
WEB_ASSET_ENV = "TAVILY_HIKARI_WEB_DIST_DIR"
RCGU_ENTRY_THRESHOLD = 50_000
RCGU_FILE_THRESHOLD = 20_000
_RCGU_PRUNE_DONE = False


def load_manifest():
    with MANIFEST_PATH.open("r", encoding="utf-8") as fh:
        manifest = json.load(fh)

    targets = manifest["coverage_targets"]
    shards = manifest["shards"]
    shard_ids = set()

    for shard in shards:
        shard_id = shard["id"]
        if shard_id in shard_ids:
            raise SystemExit(f"duplicate shard id: {shard_id}")
        shard_ids.add(shard_id)

        if shard["coverage_target"] not in targets:
            raise SystemExit(
                f"shard {shard_id} references unknown coverage target {shard['coverage_target']}"
            )

        shard.setdefault("include_prefixes", [])
        shard.setdefault("exclude_prefixes", [])
        shard.setdefault("serial_prefixes", [])
        shard.setdefault("isolated_prefixes", [])
        shard.setdefault("filtered_test_threads", 1)
        shard.setdefault("filtered_process_workers", 3)
        estimated_seconds = shard.get("estimated_seconds")
        if not isinstance(estimated_seconds, (int, float)) or estimated_seconds <= 0:
            raise SystemExit(f"shard {shard_id} needs a positive estimated_seconds value")

        if not shard["include_prefixes"] and not shard["exclude_prefixes"]:
            shard["mode"] = "all"
        elif shard["include_prefixes"]:
            shard["mode"] = "include"
        else:
            shard["mode"] = "exclude"

    return targets, shards


def cargo_test_command(args, cargo_profile=None):
    command = list(BASE_CARGO_ARGS)
    if cargo_profile:
        command.extend(["--profile", cargo_profile])
    command.extend(args)
    return command


def cargo_environment(cargo_jobs=None, web_assets_dir=None):
    env = os.environ.copy()
    if cargo_jobs is not None:
        env["CARGO_BUILD_JOBS"] = str(cargo_jobs)
    if web_assets_dir is not None:
        env[WEB_ASSET_ENV] = str(web_assets_dir)
    return env


def parse_test_list(stdout: str):
    tests = []
    for line in stdout.splitlines():
        if line.endswith(": test"):
            tests.append(line[:-6])
    return tests


def parse_json_lines(stdout: str):
    records = []
    for line in stdout.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            records.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return records


def parse_requested_targets(cargo_args):
    expected = {"lib": False, "bins": set(), "tests": set()}
    idx = 0
    while idx < len(cargo_args):
        arg = cargo_args[idx]
        if arg == "--lib":
            expected["lib"] = True
            idx += 1
            continue
        if arg == "--bin":
            expected["bins"].add(cargo_args[idx + 1])
            idx += 2
            continue
        if arg == "--test":
            expected["tests"].add(cargo_args[idx + 1])
            idx += 2
            continue
        idx += 1
    return expected


def artifact_target_dir_name(target_id):
    safe = []
    for char in target_id:
        if char.isalnum() or char in "._-":
            safe.append(char)
        else:
            safe.append("-")
    slug = "".join(safe).strip("._-")
    if not slug:
        slug = "target"
    if slug == target_id:
        return slug
    digest = hashlib.sha1(target_id.encode("utf-8")).hexdigest()[:8]
    return f"{slug}-{digest}"


SUPPORT_BINARIES_BY_TARGET = {
    "lib": {
        "OBSERVABILITY_LOCK_HOLDER_BIN": "observability_lock_holder",
    },
    "integration:mcp_billing_regression": {
        "TAVILY_HIKARI_TEST_BIN": "tavily-hikari",
    },
    "integration:mcp_session_affinity_e2e": {
        "TAVILY_HIKARI_TEST_BIN": "tavily-hikari",
    },
    "integration:request_kind_canonical_backfill": {
        "REQUEST_KIND_CANONICAL_BACKFILL_TEST_BIN": "request_kind_canonical_backfill",
    },
    "integration:server_http_contract": {
        "TAVILY_HIKARI_TEST_BIN": "tavily-hikari",
    },
}


def target_matches_requested(target_name, target_kind, requested):
    target_kind = set(target_kind)
    return (
        (requested["lib"] and "lib" in target_kind)
        or (target_name in requested["bins"] and "bin" in target_kind)
        or (target_name in requested["tests"] and "test" in target_kind)
    )


def combined_coverage_list_args(targets):
    combined = []
    include_lib = False
    bins = set()
    tests = set()
    for target in targets.values():
        requested = parse_requested_targets(target["list_args"])
        include_lib = include_lib or requested["lib"]
        bins.update(requested["bins"])
        tests.update(requested["tests"])

    if include_lib:
        combined.append("--lib")
    for bin_name in sorted(bins):
        combined.extend(["--bin", bin_name])
    for test_name in sorted(tests):
        combined.extend(["--test", test_name])
    return combined


def run_cargo(args, cargo_jobs=None, cargo_profile=None, web_assets_dir=None):
    cmd = cargo_test_command(args, cargo_profile=cargo_profile)
    print("+", " ".join(cmd), flush=True)
    subprocess.run(
        cmd,
        cwd=ROOT,
        check=True,
        env=cargo_environment(cargo_jobs=cargo_jobs, web_assets_dir=web_assets_dir),
    )


def maybe_prune_build_artifacts(cargo_profile=None):
    global _RCGU_PRUNE_DONE
    if _RCGU_PRUNE_DONE:
        return
    _RCGU_PRUNE_DONE = True

    profile_dir = cargo_profile or "debug"
    deps_dir = ROOT / "target" / profile_dir / "deps"
    if not deps_dir.is_dir():
        return

    total_entries = 0
    rcgu_files = 0
    with os.scandir(deps_dir) as entries:
        for entry in entries:
            total_entries += 1
            if entry.is_file() and entry.name.endswith(".rcgu.o"):
                rcgu_files += 1

    if total_entries < RCGU_ENTRY_THRESHOLD and rcgu_files < RCGU_FILE_THRESHOLD:
        return

    print(
        "pruning stale rustc objects before backend test build "
        f"(entries={total_entries}, rcgu={rcgu_files})",
        flush=True,
    )
    subprocess.run(
        [
            sys.executable,
            str(ROOT / "scripts" / "prune_rustc_artifacts.py"),
            str(ROOT / "target"),
            "--all",
        ],
        cwd=ROOT,
        check=True,
    )


def capture_test_list(list_args, cargo_jobs=None, cargo_profile=None, web_assets_dir=None):
    cmd = cargo_test_command(list_args + ["--", "--list"], cargo_profile=cargo_profile)
    try:
        completed = subprocess.run(
            cmd,
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
            timeout=CARGO_LIST_TIMEOUT_SECONDS,
            env=cargo_environment(cargo_jobs=cargo_jobs, web_assets_dir=web_assets_dir),
        )
    except subprocess.TimeoutExpired as exc:
        args = " ".join(cmd)
        raise SystemExit(f"timed out listing tests for `{args}` after {exc.timeout}s") from exc
    return parse_test_list(completed.stdout)


def capture_test_list_via_executables(
    list_args, cargo_jobs=None, cargo_profile=None, web_assets_dir=None
):
    executables = build_test_executables(
        list_args,
        cargo_jobs=cargo_jobs,
        cargo_profile=cargo_profile,
        web_assets_dir=web_assets_dir,
    )
    if not executables:
        raise SystemExit(
            f"no test executables produced while listing {' '.join(cargo_test_command(list_args, cargo_profile))}"
        )

    tests = []
    for executable in executables:
        executable_tests = list_executable_tests(executable["path"])
        if not executable_tests:
            raise SystemExit(
                f"failed to list tests from executable {executable['path']} for target {executable['name']}"
            )
        tests.extend(executable_tests)
    return sorted(set(tests))


def build_test_executables(
    cargo_args,
    include_non_test_binaries=False,
    cargo_jobs=None,
    cargo_profile=None,
    web_assets_dir=None,
):
    maybe_prune_build_artifacts(cargo_profile=cargo_profile)
    requested = parse_requested_targets(cargo_args)
    cmd = cargo_test_command(
        cargo_args + ["--no-run", "--message-format", "json"], cargo_profile=cargo_profile
    )
    completed = subprocess.run(
        cmd,
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
        env=cargo_environment(cargo_jobs=cargo_jobs, web_assets_dir=web_assets_dir),
    )
    if completed.returncode != 0:
        if completed.stdout:
            sys.stdout.write(completed.stdout)
        if completed.stderr:
            sys.stderr.write(completed.stderr)
        raise SystemExit(completed.returncode)
    executables = []
    for record in parse_json_lines(completed.stdout):
        if record.get("reason") != "compiler-artifact":
            continue
        executable = record.get("executable")
        if not executable:
            continue
        target = record.get("target", {})
        profile = record.get("profile", {})
        is_test_profile = profile.get("test", False)
        if not is_test_profile and not include_non_test_binaries:
            continue
        target_name = target.get("name")
        target_kind = target.get("kind", [])
        is_plain_binary = "bin" in target_kind and not is_test_profile
        if not is_plain_binary and not target_matches_requested(target_name, target_kind, requested):
            continue
        executables.append(
            {
                "name": target_name,
                "kind": tuple(target_kind),
                "path": executable,
                "test_profile": is_test_profile,
            }
        )
    return executables


def list_executable_tests(executable_path, timeout_seconds=EXECUTABLE_LIST_TIMEOUT_SECONDS):
    try:
        completed = subprocess.run(
            [executable_path, "--list"],
            cwd=ROOT,
            check=False,
            capture_output=True,
            text=True,
            timeout=timeout_seconds,
        )
    except subprocess.TimeoutExpired:
        return None
    if completed.returncode != 0:
        return None
    return parse_test_list(completed.stdout)


def list_tests_from_executables(executables):
    tests = []
    for executable in executables:
        executable_tests = executable.get("tests")
        if executable_tests is None:
            executable_tests = list_executable_tests(
                executable["path"], EXECUTABLE_LIST_RETRY_TIMEOUT_SECONDS
            )
        if executable_tests is None:
            raise SystemExit(
                f"failed to list tests from executable {executable['path']} for target {executable['name']}"
            )
        tests.extend(executable_tests)
    return sorted(set(tests))


def run_exact_tests(executable_path, selected_tests, process_workers=None):
    run_exact_tests_with_env(executable_path, selected_tests, process_workers=process_workers)


def run_exact_tests_with_env(
    executable_path, selected_tests, extra_env=None, process_workers=None
):
    if not selected_tests:
        return

    batches = [[test_name] for test_name in selected_tests]
    run_parallel_test_commands(
        [
            [executable_path, "--exact", "--test-threads=1", *batch]
            for batch in batches
        ],
        max_workers=min(process_workers or 6, len(batches)),
        extra_env=extra_env,
    )


def run_filtered_tests(
    executable_path, filters, test_threads, process_workers, extra_env=None
):
    run_filtered_tests_with_env(
        executable_path,
        filters,
        test_threads,
        process_workers,
        extra_env=extra_env,
    )


def run_filtered_tests_with_env(
    executable_path, filters, test_threads, process_workers, extra_env=None
):
    if not filters:
        return

    # Keep each prefix in its own rust test process. Running the prefixes in
    # parallel is safe because each invocation gets its own process-global env,
    # temp dir state, and sqlite connections.
    commands = [
        [executable_path, f"--test-threads={test_threads}", filter_name]
        for filter_name in filters
    ]
    run_parallel_test_commands(
        commands,
        max_workers=min(process_workers, len(commands)),
        extra_env=extra_env,
    )


def run_parallel_test_commands(commands, max_workers, extra_env=None):
    if not commands:
        return

    worker_count = max(1, min(max_workers, len(commands)))
    started = time.monotonic()
    with concurrent.futures.ThreadPoolExecutor(max_workers=worker_count) as executor:
        future_to_command = {
            executor.submit(
                subprocess.run,
                command,
                cwd=ROOT,
                capture_output=True,
                text=True,
                env=extra_env,
            ): command
            for command in commands
        }
        for future in concurrent.futures.as_completed(future_to_command):
            command = future_to_command[future]
            completed = future.result()
            elapsed = time.monotonic() - started
            print(
                f"done command={command[-1]} rc={completed.returncode} elapsed={elapsed:.2f}s",
                flush=True,
            )
            if completed.stdout:
                sys.stdout.write(completed.stdout)
            if completed.stderr:
                sys.stderr.write(completed.stderr)
            if completed.returncode != 0:
                raise SystemExit(completed.returncode)


def run_all_tests(executable_path, test_threads):
    run_all_tests_with_env(executable_path, test_threads)


def run_all_tests_with_env(executable_path, test_threads, extra_env=None):
    cmd = [executable_path, f"--test-threads={test_threads}"]
    print("+", " ".join(cmd), flush=True)
    subprocess.run(cmd, cwd=ROOT, check=True, env=extra_env)


def ensure_executable(path):
    path = Path(path)
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    return path


WEB_ASSET_FIXTURE_FILES = {
    "index.html": "<!doctype html><html><head><title>index</title></head><body></body></html>\n",
    "console.html": "<!doctype html><html><head><title>console</title></head><body></body></html>\n",
    "admin.html": "<!doctype html><html><head><title>admin</title></head><body></body></html>\n",
    "login.html": "<!doctype html><html><head><title>login</title></head><body></body></html>\n",
    "registration-paused.html": "<!doctype html><html><head><title>registration-paused</title></head><body></body></html>\n",
    "version.json": '{"version":"ci-test"}\n',
    "favicon.svg": '<svg xmlns="http://www.w3.org/2000/svg"><image href="assets/relay-mesh-mark-light.png"/></svg>\n',
    "assets/linuxdo-logo.svg": '<svg xmlns="http://www.w3.org/2000/svg"></svg>\n',
    "assets/relay-mesh-lockup-light.svg": '<svg xmlns="http://www.w3.org/2000/svg"></svg>\n',
    "assets/relay-mesh-lockup-dark.svg": '<svg xmlns="http://www.w3.org/2000/svg"></svg>\n',
    "assets/relay-mesh-mobile-logo-light.svg": '<svg xmlns="http://www.w3.org/2000/svg"></svg>\n',
    "assets/relay-mesh-mobile-logo-dark.svg": '<svg xmlns="http://www.w3.org/2000/svg"></svg>\n',
    "assets/relay-mesh-mark-light.svg": '<svg xmlns="http://www.w3.org/2000/svg"></svg>\n',
    "assets/relay-mesh-mark-light.png": "ci-test-png\n",
    "assets/relay-mesh-lockup-light.png": "ci-test-png\n",
}


def write_minimal_web_assets(output_dir):
    output_dir = Path(output_dir)
    for relative_path, contents in WEB_ASSET_FIXTURE_FILES.items():
        path = output_dir / relative_path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents, encoding="utf-8")
    return output_dir


def verify_web_assets(root):
    root = Path(root)
    missing = []
    for relative_path in WEB_ASSET_FIXTURE_FILES:
        path = root / relative_path
        if not path.is_file() or path.stat().st_size == 0:
            missing.append(relative_path)
    if missing:
        raise SystemExit(f"web asset contract missing: {', '.join(missing)}")

    version_path = root / "version.json"
    try:
        version = json.loads(version_path.read_text(encoding="utf-8"))["version"]
    except (json.JSONDecodeError, KeyError, TypeError) as exc:
        raise SystemExit(f"invalid version.json at {version_path}") from exc
    if not isinstance(version, str) or not version:
        raise SystemExit(f"version.json at {version_path} needs a non-empty version")


def sha256_file(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as fh:
        for chunk in iter(lambda: fh.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def artifact_executable_path(output_dir, digest, source_name):
    safe_name = "".join(
        char if char.isalnum() or char in ".-_" else "-" for char in source_name
    ).strip(".-_")
    if not safe_name:
        safe_name = "executable"
    # Keep the source name first so Rust helpers that resolve sibling binaries
    # by an exact name or ``name-*`` prefix work without duplicate bundle files.
    return Path(output_dir) / "executables" / f"{safe_name}-{digest}"


def stage_lane_runner(output_dir):
    output_dir = Path(output_dir)
    runtime_scripts = output_dir / "scripts"
    runtime_scripts.mkdir(parents=True, exist_ok=True)
    shutil.copy2(Path(__file__).resolve(), runtime_scripts / "ci_backend_tests.py")
    shutil.copy2(MANIFEST_PATH, runtime_scripts / MANIFEST_PATH.name)
    source_snapshot = output_dir / "source"
    for source_dir in ("src", "tests"):
        shutil.copytree(ROOT / source_dir, source_snapshot / source_dir, dirs_exist_ok=True)


def build_artifacts(output_dir, cargo_jobs=None, cargo_profile=None, web_assets="project"):
    targets, _ = load_manifest()
    output_dir = Path(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    prepare_started = time.monotonic()

    combined_args = combined_coverage_list_args(targets)
    temporary_fixture = None
    web_assets_dir = None
    if web_assets == "minimal":
        temporary_fixture = tempfile.TemporaryDirectory(prefix="ci-web-assets-")
        web_assets_dir = write_minimal_web_assets(temporary_fixture.name)
        verify_web_assets(web_assets_dir)
    elif web_assets != "project":
        raise SystemExit(f"unsupported web asset source: {web_assets}")

    try:
        build_started = time.monotonic()
        executables = build_test_executables(
            combined_args,
            include_non_test_binaries=True,
            cargo_jobs=cargo_jobs,
            cargo_profile=cargo_profile,
            web_assets_dir=web_assets_dir,
        )
        print(
            "prepare_phase_complete phase=compile_test_executables "
            f"elapsed_seconds={time.monotonic() - build_started:.2f}",
            flush=True,
        )
    finally:
        if temporary_fixture is not None:
            temporary_fixture.cleanup()
    if not executables:
        raise SystemExit("no test executables produced while preparing backend test artifacts")
    built_executables_by_target = {target_id: [] for target_id in targets}
    for executable in executables:
        if not executable.get("test_profile", False):
            continue
        for target_id, target in targets.items():
            requested = parse_requested_targets(target["list_args"])
            if target_matches_requested(executable["name"], executable["kind"], requested):
                built_executables_by_target[target_id].append(executable)

    built_support_binaries = {}
    for executable in executables:
        if "bin" not in executable["kind"] or executable.get("test_profile", True):
            continue
        built_support_binaries[executable["name"]] = executable["path"]

    _, shards = load_manifest()
    target_shards = defaultdict(list)
    for shard in shards:
        target_shards[shard["coverage_target"]].append(shard)

    executables_requiring_test_lists = []
    for target_id, executable_entries in built_executables_by_target.items():
        shards_for_target = target_shards[target_id]
        if not shards_for_target:
            continue
        needs_test_list = not (
            len(shards_for_target) == 1 and shards_for_target[0]["mode"] == "all"
        )
        if not needs_test_list:
            continue
        executables_requiring_test_lists.extend(executable_entries)

    test_list_started = time.monotonic()
    populate_executable_test_lists(executables_requiring_test_lists)
    print(
        "prepare_phase_complete phase=discover_test_lists "
        f"elapsed_seconds={time.monotonic() - test_list_started:.2f}",
        flush=True,
    )

    bundle_started = time.monotonic()
    artifact_files = {}
    source_digests = {}

    def store_executable(source_path, tests=None):
        source = Path(source_path)
        source_key = source.resolve()
        digest = source_digests.get(source_key)
        if digest is None:
            digest = sha256_file(source)
            source_digests[source_key] = digest
        entry = artifact_files.get(digest)
        if entry is None:
            destination = artifact_executable_path(output_dir, digest, source.name)
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, destination)
            ensure_executable(destination)
            entry = {
                "path": str(destination.relative_to(output_dir)),
                "sha256": digest,
                "name": source.name,
            }
            artifact_files[digest] = entry
        if tests is not None:
            existing_tests = entry.get("tests")
            if existing_tests is not None and existing_tests != tests:
                raise SystemExit(f"conflicting test metadata for executable {source}")
            entry["tests"] = tests
        return digest

    coverage_target_metadata = {}
    for target_id, executable_entries in built_executables_by_target.items():
        if not executable_entries:
            raise SystemExit(f"no test executables produced for coverage target {target_id}")
        target_metadata = {"executables": [], "support_binaries": {}}
        for executable in executable_entries:
            digest = store_executable(executable["path"], executable.get("tests"))
            target_metadata["executables"].append(digest)
        for env_name, binary_name in SUPPORT_BINARIES_BY_TARGET.get(target_id, {}).items():
            source_path = built_support_binaries.get(binary_name)
            if source_path is None:
                raise SystemExit(
                    f"missing support binary {binary_name} required by coverage target {target_id}"
                )
            target_metadata["support_binaries"][env_name] = store_executable(source_path)
        coverage_target_metadata[target_id] = target_metadata

    manifest = {
        "format_version": ARTIFACT_FORMAT_VERSION,
        "executables": artifact_files,
        "coverage_targets": coverage_target_metadata,
    }
    with (output_dir / ARTIFACT_MANIFEST_NAME).open("w", encoding="utf-8") as fh:
        json.dump(manifest, fh, sort_keys=True, indent=2)
        fh.write("\n")
    stage_lane_runner(output_dir)
    print(
        "prepare_phase_complete phase=stage_bundle "
        f"elapsed_seconds={time.monotonic() - bundle_started:.2f}",
        flush=True,
    )
    print(
        "prepare_complete "
        f"elapsed_seconds={time.monotonic() - prepare_started:.2f}",
        flush=True,
    )


def load_v2_prebuilt_executables(artifact_root, coverage_target):
    artifact_root = Path(artifact_root).resolve()
    manifest_path = artifact_root / ARTIFACT_MANIFEST_NAME
    with manifest_path.open("r", encoding="utf-8") as fh:
        manifest = json.load(fh)
    if manifest.get("format_version") != ARTIFACT_FORMAT_VERSION:
        raise SystemExit(f"unsupported backend artifact format in {manifest_path}")
    target_metadata = manifest.get("coverage_targets", {}).get(coverage_target)
    if target_metadata is None:
        raise SystemExit(f"missing prebuilt executables for coverage target {coverage_target}")

    def resolve_digest(digest):
        entry = manifest.get("executables", {}).get(digest)
        if not isinstance(entry, dict):
            raise SystemExit(f"missing artifact executable metadata for {digest}")
        if entry.get("sha256") != digest:
            raise SystemExit(f"artifact checksum metadata mismatch for {digest}")
        path = (artifact_root / entry.get("path", "")).resolve()
        if not path.is_relative_to(artifact_root) or not path.is_file():
            raise SystemExit(f"invalid artifact executable path for {digest}")
        if sha256_file(path) != digest:
            raise SystemExit(f"artifact checksum verification failed for {path}")
        return entry, ensure_executable(path)

    normalized = []
    for digest in target_metadata.get("executables", []):
        entry, path = resolve_digest(digest)
        normalized.append(
            {"name": entry["name"], "path": str(path), "tests": entry.get("tests")}
        )
    if not normalized:
        raise SystemExit(f"no executable files found for {coverage_target}")
    support_binaries = {}
    for env_name, digest in target_metadata.get("support_binaries", {}).items():
        _entry, path = resolve_digest(digest)
        support_binaries[env_name] = str(path)
    return normalized, support_binaries


def load_legacy_prebuilt_executables(artifact_root, coverage_target):
    target_dir = Path(artifact_root) / artifact_target_dir_name(coverage_target)
    if not target_dir.exists():
        legacy_dir = Path(artifact_root) / coverage_target
        if legacy_dir.exists():
            target_dir = legacy_dir
        else:
            raise SystemExit(f"missing prebuilt executables for coverage target {coverage_target}")

    metadata = {}
    metadata_path = target_dir / "tests.json"
    if metadata_path.exists():
        with metadata_path.open("r", encoding="utf-8") as fh:
            metadata = json.load(fh)

    support_binaries = {}
    support_binaries_path = target_dir / "support_binaries.json"
    if support_binaries_path.exists():
        with support_binaries_path.open("r", encoding="utf-8") as fh:
            support_binaries = json.load(fh)
    support_binary_names = set(support_binaries.values())

    executables = sorted(
        path
        for path in target_dir.iterdir()
        if path.is_file()
        and path.name not in {"tests.json", "support_binaries.json"}
        and path.name not in support_binary_names
    )
    if not executables:
        raise SystemExit(f"no executable files found in {target_dir}")

    normalized = []
    for path in executables:
        ensure_executable(path)
        normalized.append({"name": path.name, "path": str(path), "tests": metadata.get(path.name)})
    resolved_support_binaries = {
        env_name: str(ensure_executable(target_dir / file_name))
        for env_name, file_name in support_binaries.items()
    }
    return normalized, resolved_support_binaries


def load_prebuilt_executables(artifact_root, coverage_target):
    if (Path(artifact_root) / ARTIFACT_MANIFEST_NAME).is_file():
        return load_v2_prebuilt_executables(artifact_root, coverage_target)
    return load_legacy_prebuilt_executables(artifact_root, coverage_target)


def populate_executable_test_lists(executables):
    missing = [executable for executable in executables if executable.get("tests") is None]
    if not missing:
        return

    # Listing large Rust test executables is CPU and IO heavy. Keeping this pool
    # small avoids queueing slower binaries behind concurrent `--list`
    # processes that all fight for the same machine resources.
    worker_count = max(1, min(3, len(missing)))
    with concurrent.futures.ThreadPoolExecutor(max_workers=worker_count) as executor:
        future_to_executable = {}
        for executable in missing:
            future = executor.submit(list_executable_tests, executable["path"])
            future_to_executable[future] = executable
        for future in concurrent.futures.as_completed(future_to_executable):
            executable = future_to_executable[future]
            executable_tests = future.result()
            if executable_tests is None:
                executable_tests = list_executable_tests(
                    executable["path"], EXECUTABLE_LIST_RETRY_TIMEOUT_SECONDS
                )
            if executable_tests is None:
                raise SystemExit(
                    f"failed to list tests from executable {executable['path']} for target {executable['name']}"
                )
            executable["tests"] = executable_tests


def match_prefixes(name, include_prefixes, exclude_prefixes):
    if include_prefixes and not any(name.startswith(prefix) for prefix in include_prefixes):
        return False
    if exclude_prefixes and any(name.startswith(prefix) for prefix in exclude_prefixes):
        return False
    return True


def shard_matches(shard, tests):
    return [
        test_name
        for test_name in tests
        if match_prefixes(test_name, shard["include_prefixes"], shard["exclude_prefixes"])
    ]


def ensure_prefix_safe(prefix, tests, target_id):
    matches = [test_name for test_name in tests if test_name.startswith(prefix)]
    if not matches:
        raise SystemExit(f"prefix '{prefix}' matched no tests for {target_id}")


def validate_shard_prefixes(shard, tests, target_id):
    for prefix in shard["include_prefixes"]:
        ensure_prefix_safe(prefix, tests, target_id)
    for prefix in shard["exclude_prefixes"]:
        ensure_prefix_safe(prefix, tests, target_id)
    for prefix in shard["serial_prefixes"]:
        ensure_prefix_safe(prefix, tests, target_id)
    for prefix in shard["isolated_prefixes"]:
        ensure_prefix_safe(prefix, tests, target_id)


def select_safe_filter_groups(executable_tests, shard):
    include_prefixes = shard["include_prefixes"]
    exclude_prefixes = shard["exclude_prefixes"]
    serial_prefixes = set(shard["serial_prefixes"])
    isolated_prefixes = set(shard["isolated_prefixes"])
    selected = {
        test_name
        for test_name in executable_tests
        if match_prefixes(test_name, include_prefixes, exclude_prefixes)
    }
    if not selected:
        return [], []

    remaining = set(selected)
    safe_groups = []
    for prefix in include_prefixes:
        if prefix in isolated_prefixes:
            continue
        starts_with_prefix = {test_name for test_name in executable_tests if test_name.startswith(prefix)}
        if not starts_with_prefix:
            continue
        substring_matches = {test_name for test_name in executable_tests if prefix in test_name}
        if substring_matches != starts_with_prefix:
            continue
        if not starts_with_prefix.issubset(remaining):
            continue
        safe_groups.append((prefix, starts_with_prefix))
        remaining -= starts_with_prefix

    filters = [prefix for prefix, _ in safe_groups if prefix not in serial_prefixes]
    serial_filters = [prefix for prefix, _ in safe_groups if prefix in serial_prefixes]
    isolated_tests = sorted(
        test_name
        for test_name in remaining
        if any(test_name.startswith(prefix) for prefix in isolated_prefixes)
    )
    exact_fallback = sorted(remaining.difference(isolated_tests))
    return filters, serial_filters, exact_fallback, isolated_tests


def verify_manifest(prebuilt_root=None):
    targets, shards = load_manifest()
    shards_by_kind = defaultdict(list)
    shards_by_target = defaultdict(list)
    matched_by_target = {}

    for shard in shards:
        shards_by_kind[shard["kind"]].append({"id": shard["id"], "name": shard["name"]})
        shards_by_target[shard["coverage_target"]].append(shard)

    for target_id, target in targets.items():
        target_shards = shards_by_target[target_id]

        if len(target_shards) == 1 and target_shards[0]["mode"] == "all":
            shard = target_shards[0]
            matched_by_target[shard["id"]] = None
            print(f"{target_id}: all tests covered by {shard['id']}", flush=True)
            continue

        if prebuilt_root:
            executables, _support_binaries = load_prebuilt_executables(prebuilt_root, target_id)
            tests = list_tests_from_executables(executables)
        else:
            tests = capture_test_list_via_executables(target["list_args"])
        owners = defaultdict(list)
        shard_counts = []

        for shard in target_shards:
            validate_shard_prefixes(shard, tests, target_id)
            matched = shard_matches(shard, tests)
            matched_by_target[shard["id"]] = matched
            shard_counts.append((shard["id"], len(matched)))
            for test_name in matched:
                owners[test_name].append(shard["id"])

        unmatched = [test_name for test_name in tests if test_name not in owners]
        overlaps = {
            test_name: shard_ids
            for test_name, shard_ids in owners.items()
            if len(shard_ids) > 1
        }

        if unmatched:
            print(f"unmatched tests for {target_id}:", file=sys.stderr)
            for test_name in unmatched:
                print(f"  - {test_name}", file=sys.stderr)
            raise SystemExit(1)

        if overlaps:
            print(f"overlapping tests for {target_id}:", file=sys.stderr)
            for test_name, shard_ids in overlaps.items():
                print(f"  - {test_name}: {', '.join(shard_ids)}", file=sys.stderr)
            raise SystemExit(1)

        print(f"{target_id}: {len(tests)} tests", flush=True)
        for shard_id, count in sorted(shard_counts):
            print(f"  - {shard_id}: {count}", flush=True)

    return shards_by_kind


def output_matrix(kind):
    _, shards = load_manifest()
    matrix = [
        {
            "id": shard["id"],
            "name": shard["name"],
            "coverage_target": shard["coverage_target"],
        }
        for shard in shards
        if shard["kind"] == kind
    ]
    print(json.dumps(matrix))


def build_lane_matrix(shards, lane_count):
    if lane_count < 1:
        raise SystemExit("lane count must be at least one")
    if not shards:
        return []

    lanes = [
        {"id": f"lane-{number:02d}", "name": f"Lane {number:02d}", "estimated_seconds": 0, "shard_ids": []}
        for number in range(1, min(lane_count, len(shards)) + 1)
    ]
    for shard in sorted(shards, key=lambda item: (-item["estimated_seconds"], item["id"])):
        lane_index = min(
            range(len(lanes)), key=lambda index: (lanes[index]["estimated_seconds"], index)
        )
        lane = lanes[lane_index]
        lane["shard_ids"].append(shard["id"])
        lane["estimated_seconds"] += shard["estimated_seconds"]
    return lanes


def shard_resource_limits(shard, filtered_process_workers=None, filtered_test_threads=None):
    test_threads = (
        shard["filtered_test_threads"]
        if filtered_test_threads is None
        else min(filtered_test_threads, shard["filtered_test_threads"])
    )
    process_workers = (
        shard["filtered_process_workers"]
        if filtered_process_workers is None
        else min(filtered_process_workers, shard["filtered_process_workers"])
    )
    return process_workers, test_threads


def output_lane_matrix(lane_count):
    _, shards = load_manifest()
    print(json.dumps(build_lane_matrix(shards, lane_count), separators=(",", ":")))


def run_shard(
    shard_id,
    prebuilt_root=None,
    cargo_jobs=None,
    cargo_profile=None,
    filtered_process_workers=None,
    filtered_test_threads=None,
):
    targets, shards = load_manifest()
    shard = next((item for item in shards if item["id"] == shard_id), None)
    if shard is None:
        raise SystemExit(f"unknown shard id: {shard_id}")
    filtered_process_workers, filtered_test_threads = shard_resource_limits(
        shard,
        filtered_process_workers=filtered_process_workers,
        filtered_test_threads=filtered_test_threads,
    )

    target_id = shard["coverage_target"]
    extra_env = os.environ.copy()
    if shard["mode"] == "all":
        if prebuilt_root:
            executables, support_binaries = load_prebuilt_executables(prebuilt_root, target_id)
            extra_env.update(support_binaries)
        else:
            executables = build_test_executables(
                shard["run_args"], cargo_jobs=cargo_jobs, cargo_profile=cargo_profile
            )
        for executable in executables:
            run_all_tests_with_env(executable["path"], filtered_test_threads, extra_env=extra_env)
        return

    if prebuilt_root:
        executables, support_binaries = load_prebuilt_executables(prebuilt_root, target_id)
        extra_env.update(support_binaries)
        target_tests = list_tests_from_executables(executables)
    else:
        executables = build_test_executables(
            shard["run_args"], cargo_jobs=cargo_jobs, cargo_profile=cargo_profile
        )
        target_tests = capture_test_list_via_executables(
            targets[target_id]["list_args"], cargo_jobs=cargo_jobs, cargo_profile=cargo_profile
        )

    validate_shard_prefixes(shard, target_tests, target_id)
    selected_tests = shard_matches(shard, target_tests)
    selected_set = set(selected_tests)

    if not executables:
        raise SystemExit(f"no test executables produced for shard {shard_id}")

    for executable in executables:
        executable_tests = executable.get("tests")
        if executable_tests is None:
            executable_tests = list_executable_tests(executable["path"])
        executable_selected = [name for name in executable_tests if name in selected_set]
        if not executable_selected:
            continue

        filter_groups, serial_filter_groups, exact_fallback, isolated_tests = select_safe_filter_groups(
            executable_tests, shard
        )
        run_filtered_tests(
            executable["path"],
            filter_groups,
            filtered_test_threads,
            filtered_process_workers,
            extra_env=extra_env,
        )
        for serial_filter in serial_filter_groups:
            run_filtered_tests(
                executable["path"], [serial_filter], 1, 1, extra_env=extra_env
            )
        run_exact_tests_with_env(
            executable["path"],
            exact_fallback,
            extra_env=extra_env,
            process_workers=filtered_process_workers,
        )
        run_exact_tests_with_env(
            executable["path"],
            isolated_tests,
            extra_env=extra_env,
            process_workers=1,
        )


def run_lane(
    lane_id,
    lane_count,
    prebuilt_root=None,
    cargo_jobs=None,
    cargo_profile=None,
    filtered_process_workers=None,
    filtered_test_threads=None,
):
    _, shards = load_manifest()
    lanes = build_lane_matrix(shards, lane_count)
    lane = next((item for item in lanes if item["id"] == lane_id), None)
    if lane is None:
        raise SystemExit(f"unknown lane id: {lane_id}")
    print(
        f"lane_start id={lane_id} estimated_seconds={lane['estimated_seconds']} shards={','.join(lane['shard_ids'])}",
        flush=True,
    )
    for shard_id in lane["shard_ids"]:
        shard_started = time.monotonic()
        print(f"shard_start id={shard_id}", flush=True)
        run_shard(
            shard_id,
            prebuilt_root=prebuilt_root,
            cargo_jobs=cargo_jobs,
            cargo_profile=cargo_profile,
            filtered_process_workers=filtered_process_workers,
            filtered_test_threads=filtered_test_threads,
        )
        print(
            f"shard_complete id={shard_id} elapsed_seconds={time.monotonic() - shard_started:.2f}",
            flush=True,
        )
    print(f"lane_complete id={lane_id}", flush=True)


def run_all(
    prebuilt_root=None,
    cargo_jobs=DEFAULT_LOW_RESOURCE_CARGO_JOBS,
    cargo_profile=None,
    filtered_process_workers=DEFAULT_LOW_RESOURCE_FILTERED_PROCESS_WORKERS,
    filtered_test_threads=DEFAULT_LOW_RESOURCE_FILTERED_TEST_THREADS,
    web_assets="minimal",
):
    _, shards = load_manifest()
    if prebuilt_root:
        for shard in shards:
            run_shard(
                shard["id"],
                prebuilt_root=prebuilt_root,
                cargo_jobs=cargo_jobs,
                cargo_profile=cargo_profile,
                filtered_process_workers=filtered_process_workers,
                filtered_test_threads=filtered_test_threads,
            )
        return

    with tempfile.TemporaryDirectory(prefix="backend-test-artifacts-") as temp_dir:
        build_artifacts(
            temp_dir,
            cargo_jobs=cargo_jobs,
            cargo_profile=cargo_profile,
            web_assets=web_assets,
        )
        verify_manifest(prebuilt_root=temp_dir)
        run_all(
            prebuilt_root=temp_dir,
            cargo_jobs=cargo_jobs,
                cargo_profile=cargo_profile,
                filtered_process_workers=filtered_process_workers,
                filtered_test_threads=filtered_test_threads,
                web_assets=web_assets,
        )


def benchmark_shards(max_workers, filtered_test_threads=None):
    _, shards = load_manifest()
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="backend-test-artifacts-") as temp_dir:
        build_started = time.monotonic()
        build_artifacts(temp_dir)
        build_elapsed = time.monotonic() - build_started

        verify_started = time.monotonic()
        verify_manifest(prebuilt_root=temp_dir)
        verify_elapsed = time.monotonic() - verify_started

        shard_commands = []
        for shard in shards:
            command = [
                sys.executable,
                str(ROOT / "scripts" / "ci_backend_tests.py"),
                "run-shard",
                "--id",
                shard["id"],
                "--prebuilt-root",
                temp_dir,
            ]
            if filtered_test_threads is not None:
                command.extend(["--filtered-test-threads", str(filtered_test_threads)])
            shard_commands.append((command, shard))

        shard_started = time.monotonic()
        shard_results = []
        with concurrent.futures.ThreadPoolExecutor(max_workers=max_workers) as executor:
            future_to_shard = {
                executor.submit(
                    subprocess.run,
                    command,
                    cwd=ROOT,
                    capture_output=True,
                    text=True,
                ): shard["id"]
                for command, shard in shard_commands
                if "forward_proxy::tests::" not in shard.get("serial_prefixes", [])
            }
            for future in concurrent.futures.as_completed(future_to_shard):
                shard_id = future_to_shard[future]
                completed = future.result()
                elapsed = time.monotonic() - shard_started
                shard_results.append((shard_id, completed.returncode, elapsed))
                print(
                    f"done shard={shard_id} rc={completed.returncode} elapsed={elapsed:.2f}s",
                    flush=True,
                )
                if completed.returncode != 0:
                    if completed.stdout:
                        sys.stdout.write(completed.stdout)
                    if completed.stderr:
                        sys.stderr.write(completed.stderr)
                    raise SystemExit(completed.returncode)
        for command, shard in shard_commands:
            if "forward_proxy::tests::" not in shard.get("serial_prefixes", []):
                continue
            completed = subprocess.run(
                command,
                cwd=ROOT,
                capture_output=True,
                text=True,
            )
            elapsed = time.monotonic() - shard_started
            shard_results.append((shard["id"], completed.returncode, elapsed))
            print(
                f"done shard={shard['id']} rc={completed.returncode} elapsed={elapsed:.2f}s",
                flush=True,
            )
            if completed.returncode != 0:
                if completed.stdout:
                    sys.stdout.write(completed.stdout)
                if completed.stderr:
                    sys.stderr.write(completed.stderr)
                raise SystemExit(completed.returncode)
        shard_elapsed = time.monotonic() - shard_started

    total_elapsed = time.monotonic() - started
    print(f"prepare_artifacts_seconds={build_elapsed:.2f}")
    print(f"verify_seconds={verify_elapsed:.2f}")
    print(f"shards_seconds={shard_elapsed:.2f}")
    print(f"total_seconds={total_elapsed:.2f}")


def add_resource_arguments(parser, defaults=True):
    parser.add_argument(
        "--cargo-jobs",
        type=int,
        default=DEFAULT_LOW_RESOURCE_CARGO_JOBS if defaults else None,
    )
    parser.add_argument(
        "--filtered-process-workers",
        type=int,
        default=DEFAULT_LOW_RESOURCE_FILTERED_PROCESS_WORKERS if defaults else None,
    )
    parser.add_argument(
        "--filtered-test-threads",
        type=int,
        default=DEFAULT_LOW_RESOURCE_FILTERED_TEST_THREADS if defaults else None,
    )
    parser.add_argument("--diagnostic", action="store_true")


def resources_from_args(args):
    if args.diagnostic:
        return (
            DIAGNOSTIC_CARGO_JOBS,
            DIAGNOSTIC_FILTERED_PROCESS_WORKERS,
            DIAGNOSTIC_FILTERED_TEST_THREADS,
        )
    for option, value in (
        ("--cargo-jobs", args.cargo_jobs),
        ("--filtered-process-workers", args.filtered_process_workers),
        ("--filtered-test-threads", args.filtered_test_threads),
    ):
        if value is not None and value < 1:
            raise SystemExit(f"{option} must be at least one")
    return args.cargo_jobs, args.filtered_process_workers, args.filtered_test_threads


def main():
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)

    verify_parser = subparsers.add_parser("verify")
    verify_parser.add_argument("--prebuilt-root")

    verify_web_assets_parser = subparsers.add_parser("verify-web-assets")
    verify_web_assets_parser.add_argument("--root", required=True)

    matrix_parser = subparsers.add_parser("matrix")
    matrix_parser.add_argument("--kind", choices=["lib", "bin", "integration"], required=True)

    lane_matrix_parser = subparsers.add_parser("lane-matrix")
    lane_matrix_parser.add_argument("--lane-count", type=int, required=True)

    prepare_parser = subparsers.add_parser("prepare-artifacts")
    prepare_parser.add_argument("--output-dir", required=True)
    prepare_parser.add_argument("--cargo-profile")
    prepare_parser.add_argument("--web-assets", choices=["minimal", "project"], default="project")
    prepare_parser.add_argument("--cargo-jobs", type=int)

    benchmark_parser = subparsers.add_parser("benchmark")
    benchmark_parser.add_argument("--max-workers", type=int, default=DEFAULT_BENCHMARK_WORKERS)
    benchmark_parser.add_argument("--filtered-test-threads", type=int)

    run_parser = subparsers.add_parser("run-shard")
    run_parser.add_argument("--id", required=True)
    run_parser.add_argument("--prebuilt-root")
    run_parser.add_argument("--cargo-profile")
    add_resource_arguments(run_parser)

    lane_parser = subparsers.add_parser("run-lane")
    lane_parser.add_argument("--id", required=True)
    lane_parser.add_argument("--lane-count", type=int, required=True)
    lane_parser.add_argument("--prebuilt-root")
    lane_parser.add_argument("--cargo-profile")
    add_resource_arguments(lane_parser)

    run_all_parser = subparsers.add_parser("run-all")
    run_all_parser.add_argument("--prebuilt-root")
    run_all_parser.add_argument("--cargo-profile")
    run_all_parser.add_argument("--web-assets", choices=["minimal", "project"], default="minimal")
    add_resource_arguments(run_all_parser)

    args = parser.parse_args()

    if args.command == "verify":
        verify_manifest(prebuilt_root=args.prebuilt_root)
        return
    if args.command == "verify-web-assets":
        verify_web_assets(args.root)
        return
    if args.command == "matrix":
        output_matrix(args.kind)
        return
    if args.command == "lane-matrix":
        output_lane_matrix(args.lane_count)
        return
    if args.command == "prepare-artifacts":
        if args.cargo_jobs is not None and args.cargo_jobs < 1:
            raise SystemExit("--cargo-jobs must be at least one")
        build_artifacts(
            args.output_dir,
            cargo_jobs=args.cargo_jobs,
            cargo_profile=args.cargo_profile,
            web_assets=args.web_assets,
        )
        return
    if args.command == "benchmark":
        benchmark_shards(
            max_workers=args.max_workers,
            filtered_test_threads=args.filtered_test_threads,
        )
        return
    if args.command == "run-shard":
        cargo_jobs, filtered_process_workers, filtered_test_threads = resources_from_args(args)
        run_shard(
            args.id,
            prebuilt_root=args.prebuilt_root,
            cargo_jobs=cargo_jobs,
            cargo_profile=args.cargo_profile,
            filtered_process_workers=filtered_process_workers,
            filtered_test_threads=filtered_test_threads,
        )
        return
    if args.command == "run-lane":
        cargo_jobs, filtered_process_workers, filtered_test_threads = resources_from_args(args)
        run_lane(
            args.id,
            args.lane_count,
            prebuilt_root=args.prebuilt_root,
            cargo_jobs=cargo_jobs,
            cargo_profile=args.cargo_profile,
            filtered_process_workers=filtered_process_workers,
            filtered_test_threads=filtered_test_threads,
        )
        return
    if args.command == "run-all":
        cargo_jobs, filtered_process_workers, filtered_test_threads = resources_from_args(args)
        run_all(
            prebuilt_root=args.prebuilt_root,
            cargo_jobs=cargo_jobs,
            cargo_profile=args.cargo_profile,
            filtered_process_workers=filtered_process_workers,
            filtered_test_threads=filtered_test_threads,
            web_assets=args.web_assets,
        )
        return

    raise SystemExit(f"unsupported command: {args.command}")


if __name__ == "__main__":
    main()
