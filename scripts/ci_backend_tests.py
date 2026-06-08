#!/usr/bin/env python3

import argparse
import json
import subprocess
import sys
from collections import defaultdict
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
MANIFEST_PATH = ROOT / "scripts" / "ci_backend_test_manifest.json"
BASE_CARGO_ARGS = ["cargo", "test", "--locked", "--all-features"]
CARGO_LIST_TIMEOUT_SECONDS = 300


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


def run_cargo(args):
    cmd = BASE_CARGO_ARGS + args
    print("+", " ".join(cmd), flush=True)
    subprocess.run(cmd, cwd=ROOT, check=True)


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


def build_test_executables(cargo_args):
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
        if not (target.get("test") or profile.get("test")):
            continue
        executables.append(
            {
                "name": target.get("name"),
                "kind": tuple(target.get("kind", [])),
                "path": executable,
            }
        )
    return executables


def list_executable_tests(executable_path):
    try:
        completed = subprocess.run(
            [executable_path, "--list"],
            cwd=ROOT,
            check=False,
            capture_output=True,
            text=True,
            timeout=10,
        )
    except subprocess.TimeoutExpired:
        return []
    if completed.returncode != 0:
        return []
    return parse_test_list(completed.stdout)


def run_exact_tests(executable_path, selected_tests):
    if not selected_tests:
        return

    for test_name in selected_tests:
        cmd = [executable_path, "--exact", "--test-threads=1", test_name]
        print("+", " ".join(cmd), flush=True)
        subprocess.run(cmd, cwd=ROOT, check=True)


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


def verify_manifest():
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

        tests = capture_test_list(target["list_args"])
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
        {"id": shard["id"], "name": shard["name"]}
        for shard in shards
        if shard["kind"] == kind
    ]
    print(json.dumps(matrix))


def run_shard(shard_id):
    targets, shards = load_manifest()
    shard = next((item for item in shards if item["id"] == shard_id), None)
    if shard is None:
        raise SystemExit(f"unknown shard id: {shard_id}")

    if shard["mode"] == "all":
        run_cargo(shard["run_args"])
        return

    target_id = shard["coverage_target"]
    target_tests = capture_test_list(targets[target_id]["list_args"])
    validate_shard_prefixes(shard, target_tests, target_id)
    selected_tests = shard_matches(shard, target_tests)

    executables = build_test_executables(shard["run_args"])
    selected_set = set(selected_tests)

    if not executables:
        raise SystemExit(f"no test executables produced for shard {shard_id}")

    for executable in executables:
        executable_tests = list_executable_tests(executable["path"])
        executable_selected = [name for name in executable_tests if name in selected_set]
        if shard["mode"] == "all":
            run_exact_tests(executable["path"], executable_selected)
            continue

        if executable_selected:
            run_exact_tests(executable["path"], executable_selected)


def main():
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)

    subparsers.add_parser("verify")

    matrix_parser = subparsers.add_parser("matrix")
    matrix_parser.add_argument("--kind", choices=["lib", "bin", "integration"], required=True)

    run_parser = subparsers.add_parser("run-shard")
    run_parser.add_argument("--id", required=True)

    args = parser.parse_args()

    if args.command == "verify":
        verify_manifest()
        return
    if args.command == "matrix":
        output_matrix(args.kind)
        return
    if args.command == "run-shard":
        run_shard(args.id)
        return

    raise SystemExit(f"unsupported command: {args.command}")


if __name__ == "__main__":
    main()
