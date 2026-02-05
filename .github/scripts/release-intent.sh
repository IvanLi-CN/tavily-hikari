#!/usr/bin/env bash
set -euo pipefail

# Decide whether a main commit should produce a release, based on the merged PR's intent label.
#
# This script is designed for GitHub Actions:
# - Given a commit SHA, resolve associated PR via GitHub API: /commits/{sha}/pulls
# - Require exactly 1 PR (safer default). Otherwise: conservative skip.
# - Read the PR labels and require exactly one allowed intent label:
#     type:docs|type:skip|type:patch|type:minor|type:major
# - Map to:
#     type:patch -> should_release=true, bump_level=patch
#     type:minor -> should_release=true, bump_level=minor
#     type:major -> should_release=true, bump_level=major
#     type:docs|type:skip -> should_release=false
#
# Outputs (via $GITHUB_OUTPUT when present):
# - should_release=true|false
# - bump_level=major|minor|patch|"" (empty when should_release=false)
# - release_intent_label=<label or empty>
# - pr_number=<number or empty>
# - pr_url=<url or empty>
# - reason=<string>

api_root="${GITHUB_API_URL:-https://api.github.com}"
repo="${GITHUB_REPOSITORY:-}"
token="${GITHUB_TOKEN:-}"
sha="${WORKFLOW_RUN_SHA:-${GITHUB_SHA:-}}"

if [[ -z "${repo}" ]]; then
  echo "release-intent: missing GITHUB_REPOSITORY" >&2
  exit 2
fi

if [[ -z "${sha}" ]]; then
  echo "release-intent: missing WORKFLOW_RUN_SHA (or GITHUB_SHA)" >&2
  exit 2
fi

if [[ -z "${token}" ]]; then
  echo "release-intent: missing GITHUB_TOKEN" >&2
  exit 2
fi

allowed_labels=("type:docs" "type:skip" "type:patch" "type:minor" "type:major")
allowed_channels=("channel:stable" "channel:rc")

conservative_skip() {
  local reason="$1"
  echo "should_release=false"
  echo "bump_level="
  echo "release_intent_label="
  echo "release_channel="
  echo "pr_number="
  echo "pr_url="
  echo "reason=${reason}"

  if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
    {
      echo "should_release=false"
      echo "bump_level="
      echo "release_intent_label="
      echo "release_channel="
      echo "pr_number="
      echo "pr_url="
      echo "reason=${reason}"
    } >> "${GITHUB_OUTPUT}"
  fi
}

pulls_json=""
if ! pulls_json="$(
  curl -fsSL \
    --max-time 15 \
    -H "Accept: application/vnd.github+json" \
    -H "Authorization: Bearer ${token}" \
    -H "X-GitHub-Api-Version: 2022-11-28" \
    "${api_root}/repos/${repo}/commits/${sha}/pulls?per_page=100"
)"; then
  echo "::warning::release-intent: GitHub API failed while mapping commit to PR (sha=${sha}); conservative skip"
  conservative_skip "api_failure:commit_pulls"
  exit 0
fi

export pulls_json
result="$(
  python3 - <<'PY'
from __future__ import annotations

import json
import os
import sys

pulls = json.loads(os.environ["pulls_json"])
if not isinstance(pulls, list):
    print("count=0")
    sys.exit(0)

count = len(pulls)
print(f"count={count}")
if count != 1:
    sys.exit(0)

pr = pulls[0]
pr_number = pr.get("number")
pr_url = pr.get("html_url", "")
if not isinstance(pr_number, int):
    sys.exit(0)

print(f"pr_number={pr_number}")
print(f"pr_url={pr_url}")
PY
)"

count="$(echo "$result" | sed -n 's/^count=//p')"
pr_number="$(echo "$result" | sed -n 's/^pr_number=//p')"
pr_url="$(echo "$result" | sed -n 's/^pr_url=//p')"

if [[ "${count}" != "1" ]]; then
  echo "::notice::release-intent: commit ${sha} maps to ${count:-0} PR(s); conservative skip"
  conservative_skip "ambiguous_or_missing_pr(count=${count:-0})"
  exit 0
fi

labels_json=""
if ! labels_json="$(
  curl -fsSL \
    --max-time 15 \
    -H "Accept: application/vnd.github+json" \
    -H "Authorization: Bearer ${token}" \
    -H "X-GitHub-Api-Version: 2022-11-28" \
    "${api_root}/repos/${repo}/issues/${pr_number}/labels?per_page=100"
)"; then
  echo "::warning::release-intent: GitHub API failed while reading PR labels (pr=${pr_number}); conservative skip"
  conservative_skip "api_failure:pr_labels"
  exit 0
fi

export labels_json
decision="$(
  python3 - <<'PY'
from __future__ import annotations

import json
import os
import sys

allowed = {
    "type:docs",
    "type:skip",
    "type:patch",
    "type:minor",
    "type:major",
}

allowed_channels = {
    "channel:stable",
    "channel:rc",
}

labels = json.loads(os.environ["labels_json"])
names = [l.get("name", "") for l in labels if isinstance(l, dict)]
type_like = {n for n in names if n.startswith("type:")}
unknown_type = sorted({n for n in type_like if n not in allowed})
present = sorted({n for n in names if n in allowed})

channel_like = {n for n in names if n.startswith("channel:")}
unknown_channel = sorted({n for n in channel_like if n not in allowed_channels})
present_channel = sorted({n for n in names if n in allowed_channels})

if unknown_channel:
    print("should_release=false")
    print("bump_level=")
    print("release_intent_label=")
    print("release_channel=")
    print(f"reason=unknown_channel_label({','.join(unknown_channel)})")
    sys.exit(0)

if len(present_channel) != 1:
    print("should_release=false")
    print("bump_level=")
    print("release_intent_label=")
    print("release_channel=")
    if len(present_channel) == 0:
        print("reason=missing_channel_label")
    else:
        print(f"reason=invalid_channel_label_count({len(present_channel)})")
    sys.exit(0)

channel_label = present_channel[0]
release_channel = "rc" if channel_label == "channel:rc" else "stable"

if unknown_type:
    print("should_release=false")
    print("bump_level=")
    print("release_intent_label=")
    print(f"release_channel={release_channel}")
    print(f"reason=unknown_intent_label({','.join(unknown_type)})")
    sys.exit(0)

if len(present) != 1:
    print("should_release=false")
    print("bump_level=")
    print("release_intent_label=")
    print(f"release_channel={release_channel}")
    print(f"reason=invalid_intent_label_count({len(present)})")
    sys.exit(0)

label = present[0]
if label in ("type:docs", "type:skip"):
    print("should_release=false")
    print("bump_level=")
    print(f"release_intent_label={label}")
    print(f"release_channel={release_channel}")
    print("reason=intent_skip")
    sys.exit(0)

bump_level = label.removeprefix("type:")
print("should_release=true")
print(f"bump_level={bump_level}")
print(f"release_intent_label={label}")
print(f"release_channel={release_channel}")
print("reason=intent_release")
PY
)"

should_release="$(echo "$decision" | sed -n 's/^should_release=//p')"
bump_level="$(echo "$decision" | sed -n 's/^bump_level=//p')"
intent_label="$(echo "$decision" | sed -n 's/^release_intent_label=//p')"
release_channel="$(echo "$decision" | sed -n 's/^release_channel=//p')"
reason="$(echo "$decision" | sed -n 's/^reason=//p')"

echo "Release intent decision:"
echo "  sha=${sha}"
echo "  pr_number=${pr_number}"
echo "  intent_label=${intent_label:-<none>}"
echo "  release_channel=${release_channel:-<none>}"
echo "  should_release=${should_release}"
echo "  bump_level=${bump_level:-<none>}"
echo "  reason=${reason}"

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  {
    echo "should_release=${should_release}"
    echo "bump_level=${bump_level}"
    echo "release_intent_label=${intent_label}"
    echo "release_channel=${release_channel}"
    echo "pr_number=${pr_number}"
    echo "pr_url=${pr_url}"
    echo "reason=${reason}"
  } >> "${GITHUB_OUTPUT}"
fi
