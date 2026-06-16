#!/usr/bin/env python3

from __future__ import annotations

import argparse
import os
import sys
import time
from pathlib import Path


DEFAULT_STALE_AFTER_SECS = 90
MAX_SCAN_ENTRIES = 600_000
MARKER_INTERVAL_SECS = 60


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("target_dir")
    parser.add_argument("--all", action="store_true", dest="delete_all")
    parser.add_argument("--stale-after-secs", type=int, default=DEFAULT_STALE_AFTER_SECS)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    target_dir = Path(args.target_dir)
    deps_dir = target_dir / "debug" / "deps"
    if not deps_dir.is_dir():
        return 0

    marker = target_dir / ".codex-rustc-prune.stamp"
    now = time.time()
    if not args.delete_all and marker.exists() and now - marker.stat().st_mtime < MARKER_INTERVAL_SECS:
        return 0

    total_entries = 0
    stale_cutoff = now - args.stale_after_secs
    removed = 0

    try:
        for entry in os.scandir(deps_dir):
            total_entries += 1
            if total_entries > MAX_SCAN_ENTRIES:
                break
            if not entry.is_file():
                continue
            if not entry.name.endswith(".rcgu.o"):
                continue
            try:
                stat = entry.stat(follow_symlinks=False)
            except FileNotFoundError:
                continue
            if not args.delete_all and stat.st_mtime >= stale_cutoff:
                continue
            try:
                os.unlink(entry.path)
                removed += 1
            except FileNotFoundError:
                continue
    finally:
        if not args.delete_all:
            marker.parent.mkdir(parents=True, exist_ok=True)
            marker.touch()

    if removed:
        print(
            f"[codex-rustc-prune] removed {removed} stale rcgu objects from {deps_dir}",
            file=sys.stderr,
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
