#!/usr/bin/env python3
from __future__ import annotations

import argparse
import fnmatch
import subprocess
from pathlib import Path


MATCH_PATTERNS = (
    ".bun-version",
    "docs-site/**",
    "web/**",
    ".github/workflows/docs-pages.yml",
    ".github/scripts/assemble-pages-site.sh",
    ".github/scripts/detect_docs_pages_changes.py",
    "README.md",
    "README.zh-CN.md",
)


def git_diff_names(repo_root: Path, base_sha: str, head_sha: str) -> list[str]:
    result = subprocess.run(
        ["git", "diff", "--name-only", base_sha, head_sha],
        cwd=repo_root,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        stderr = result.stderr.strip() or result.stdout.strip() or f"exit {result.returncode}"
        raise RuntimeError(f"git diff --name-only {base_sha} {head_sha} failed: {stderr}")
    return [line.strip() for line in result.stdout.splitlines() if line.strip()]


def detect_changed_files(
    repo_root: Path,
    event_name: str,
    base_sha: str | None,
    head_sha: str | None,
) -> tuple[bool, list[str], list[str]]:
    if event_name == "workflow_dispatch":
        return True, ["workflow_dispatch"], ["workflow_dispatch"]

    if not base_sha or not head_sha:
        raise RuntimeError(f"missing base/head sha for event {event_name}")

    changed_files = git_diff_names(repo_root, base_sha, head_sha)
    matched = [
        path
        for path in changed_files
        if any(fnmatch.fnmatch(path, pattern) for pattern in MATCH_PATTERNS)
    ]
    return bool(matched), changed_files, matched


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Detect whether Docs Pages should run its heavy jobs for the current event."
    )
    parser.add_argument("--repo-root", default=".")
    parser.add_argument("--event-name", required=True)
    parser.add_argument("--base-sha")
    parser.add_argument("--head-sha")
    parser.add_argument(
        "--output-file",
        help="GitHub Actions output file. When omitted, results are printed to stdout.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    repo_root = Path(args.repo_root).resolve()
    run_docs, changed_files, matched = detect_changed_files(
        repo_root,
        args.event_name,
        args.base_sha,
        args.head_sha,
    )
    payload = {
        "run_docs": "true" if run_docs else "false",
        "changed_count": str(len(changed_files)),
        "matched_paths": ",".join(matched),
    }
    lines = [f"{key}={value}" for key, value in payload.items()]
    if args.output_file:
        Path(args.output_file).write_text("\n".join(lines) + "\n", encoding="utf-8")
    else:
        print("\n".join(lines))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
