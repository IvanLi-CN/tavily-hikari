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
        shard.setdefault("filtered_test_threads", 1)
        shard.setdefault("filtered_process_workers", 3)

        if not shard["include_prefixes"] and not shard["exclude_prefixes"]:
            shard["mode"] = "all"
        elif shard["include_prefixes"]:
            shard["mode"] = "include"
        else:
            shard["mode"] = "exclude"

    return targets, shards


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


def run_cargo(args):
    cmd = BASE_CARGO_ARGS + args
    print("+", " ".join(cmd), flush=True)
    subprocess.run(cmd, cwd=ROOT, check=True)


def maybe_prune_build_artifacts():
    global _RCGU_PRUNE_DONE
    if _RCGU_PRUNE_DONE:
        return
    _RCGU_PRUNE_DONE = True

    deps_dir = ROOT / "target" / "debug" / "deps"
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


def capture_test_list(list_args):
    cmd = BASE_CARGO_ARGS + list_args + ["--", "--list"]
    try:
        completed = subprocess.run(
            cmd,
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
            timeout=CARGO_LIST_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired as exc:
        args = " ".join(cmd)
        raise SystemExit(f"timed out listing tests for `{args}` after {exc.timeout}s") from exc
    return parse_test_list(completed.stdout)


def capture_test_list_via_executables(list_args):
    executables = build_test_executables(list_args)
    if not executables:
        raise SystemExit(
            f"no test executables produced while listing {' '.join(BASE_CARGO_ARGS + list_args)}"
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


def build_test_executables(cargo_args, include_non_test_binaries=False):
    maybe_prune_build_artifacts()
    requested = parse_requested_targets(cargo_args)
    cmd = BASE_CARGO_ARGS + cargo_args + ["--no-run", "--message-format", "json"]
    completed = subprocess.run(
        cmd,
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
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


def run_exact_tests(executable_path, selected_tests):
    run_exact_tests_with_env(executable_path, selected_tests)


def run_exact_tests_with_env(executable_path, selected_tests, extra_env=None):
    if not selected_tests:
        return

    batches = [[test_name] for test_name in selected_tests]
    run_parallel_test_commands(
        [
            [executable_path, "--exact", "--test-threads=1", *batch]
            for batch in batches
        ],
        max_workers=min(6, len(batches)),
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


def build_artifacts(output_dir):
    targets, _ = load_manifest()
    output_dir = Path(output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    combined_args = combined_coverage_list_args(targets)
    executables = build_test_executables(combined_args, include_non_test_binaries=True)
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

    populate_executable_test_lists(executables_requiring_test_lists)

    for target_id, executable_entries in built_executables_by_target.items():
        if not executable_entries:
            raise SystemExit(f"no test executables produced for coverage target {target_id}")
        target_dir = output_dir / artifact_target_dir_name(target_id)
        target_dir.mkdir(parents=True, exist_ok=True)
        metadata = {}
        support_binary_metadata = {}
        for executable in executable_entries:
            source = Path(executable["path"])
            destination = target_dir / source.name
            shutil.copy2(source, destination)
            ensure_executable(destination)
            executable_tests = executable.get("tests")
            if executable_tests is not None:
                metadata[destination.name] = executable_tests
        for env_name, binary_name in SUPPORT_BINARIES_BY_TARGET.get(target_id, {}).items():
            source_path = built_support_binaries.get(binary_name)
            if source_path is None:
                raise SystemExit(
                    f"missing support binary {binary_name} required by coverage target {target_id}"
                )
            source = Path(source_path)
            destination = target_dir / source.name
            if not destination.exists():
                shutil.copy2(source, destination)
                ensure_executable(destination)
            support_binary_metadata[env_name] = destination.name
        with (target_dir / "tests.json").open("w", encoding="utf-8") as fh:
            json.dump(metadata, fh, sort_keys=True)
        with (target_dir / "support_binaries.json").open("w", encoding="utf-8") as fh:
            json.dump(support_binary_metadata, fh, sort_keys=True)


def load_prebuilt_executables(artifact_root, coverage_target):
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


def select_safe_filter_groups(executable_tests, shard):
    include_prefixes = shard["include_prefixes"]
    exclude_prefixes = shard["exclude_prefixes"]
    serial_prefixes = set(shard["serial_prefixes"])
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
    exact_fallback = sorted(remaining)
    return filters, serial_filters, exact_fallback


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


def run_shard(shard_id, prebuilt_root=None, filtered_test_threads=None):
    targets, shards = load_manifest()
    shard = next((item for item in shards if item["id"] == shard_id), None)
    if shard is None:
        raise SystemExit(f"unknown shard id: {shard_id}")
    if filtered_test_threads is None:
        filtered_test_threads = shard["filtered_test_threads"]
    filtered_process_workers = shard["filtered_process_workers"]

    target_id = shard["coverage_target"]
    extra_env = os.environ.copy()
    if shard["mode"] == "all":
        if prebuilt_root:
            executables, support_binaries = load_prebuilt_executables(prebuilt_root, target_id)
            extra_env.update(support_binaries)
        else:
            executables = build_test_executables(shard["run_args"])
        for executable in executables:
            run_all_tests_with_env(executable["path"], filtered_test_threads, extra_env=extra_env)
        return

    if prebuilt_root:
        executables, support_binaries = load_prebuilt_executables(prebuilt_root, target_id)
        extra_env.update(support_binaries)
        target_tests = list_tests_from_executables(executables)
    else:
        executables = build_test_executables(shard["run_args"])
        target_tests = capture_test_list_via_executables(targets[target_id]["list_args"])

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

        filter_groups, serial_filter_groups, exact_fallback = select_safe_filter_groups(
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
        run_exact_tests_with_env(executable["path"], exact_fallback, extra_env=extra_env)


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


def main():
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)

    verify_parser = subparsers.add_parser("verify")
    verify_parser.add_argument("--prebuilt-root")

    matrix_parser = subparsers.add_parser("matrix")
    matrix_parser.add_argument("--kind", choices=["lib", "bin", "integration"], required=True)

    prepare_parser = subparsers.add_parser("prepare-artifacts")
    prepare_parser.add_argument("--output-dir", required=True)

    benchmark_parser = subparsers.add_parser("benchmark")
    benchmark_parser.add_argument("--max-workers", type=int, default=DEFAULT_BENCHMARK_WORKERS)
    benchmark_parser.add_argument("--filtered-test-threads", type=int)

    run_parser = subparsers.add_parser("run-shard")
    run_parser.add_argument("--id", required=True)
    run_parser.add_argument("--prebuilt-root")
    run_parser.add_argument("--filtered-test-threads", type=int)

    args = parser.parse_args()

    if args.command == "verify":
        verify_manifest(prebuilt_root=args.prebuilt_root)
        return
    if args.command == "matrix":
        output_matrix(args.kind)
        return
    if args.command == "prepare-artifacts":
        build_artifacts(args.output_dir)
        return
    if args.command == "benchmark":
        benchmark_shards(
            max_workers=args.max_workers,
            filtered_test_threads=args.filtered_test_threads,
        )
        return
    if args.command == "run-shard":
        run_shard(
            args.id,
            prebuilt_root=args.prebuilt_root,
            filtered_test_threads=args.filtered_test_threads,
        )
        return

    raise SystemExit(f"unsupported command: {args.command}")


if __name__ == "__main__":
    main()
