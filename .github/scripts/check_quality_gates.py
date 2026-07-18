#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any


@dataclass
class WorkflowSpec:
    path: Path
    name: str
    pr_capable: bool
    jobs: dict[str, str]


class ValidationError(Exception):
    pass


def load_contract(path: Path) -> dict[str, Any]:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise ValidationError(f"missing contract file: {path}") from exc
    except json.JSONDecodeError as exc:
        raise ValidationError(
            f"{path}: invalid JSON at line {exc.lineno}, column {exc.colno}: {exc.msg}"
        ) from exc


def parse_scalar(raw: str) -> str:
    value = raw.split(" #", 1)[0].strip()
    if len(value) >= 2 and value[0] == value[-1] and value[0] in {'"', "'"}:
        return value[1:-1]
    return value


def parse_inline_event_names(raw: str) -> set[str]:
    value = parse_scalar(raw)
    if not value:
        return set()

    if value.startswith("[") and value.endswith("]"):
        return {
            token.strip()
            for token in value[1:-1].split(",")
            if token.strip() in {"pull_request", "pull_request_target"}
        }

    if value.startswith("{") and value.endswith("}"):
        event_names: set[str] = set()
        for item in value[1:-1].split(","):
            key = item.split(":", 1)[0].strip()
            if key in {"pull_request", "pull_request_target"}:
                event_names.add(key)
        return event_names

    if value in {"pull_request", "pull_request_target"}:
        return {value}

    return set()


def parse_workflow(path: Path) -> WorkflowSpec:
    lines = path.read_text(encoding="utf-8").splitlines()
    workflow_name: str | None = None
    pr_capable = False
    jobs: dict[str, str] = {}
    in_jobs = False
    in_on = False
    current_job: str | None = None

    for raw_line in lines:
        stripped = raw_line.strip()
        if not stripped or stripped.startswith("#"):
            continue

        indent = len(raw_line) - len(raw_line.lstrip(" "))
        if indent == 0 and raw_line.startswith("name:"):
            workflow_name = parse_scalar(raw_line.split(":", 1)[1])
            continue

        if indent == 0 and raw_line.startswith("on:"):
            inline_events = parse_inline_event_names(raw_line.split(":", 1)[1])
            pr_capable = pr_capable or bool(inline_events)
            in_on = not inline_events and raw_line.split(":", 1)[1].strip() == ""
            continue

        if in_on:
            if indent == 0:
                in_on = False
            elif indent == 2:
                event_name = stripped.split(":", 1)[0].strip()
                if event_name in {"pull_request", "pull_request_target"}:
                    pr_capable = True
                continue

        if indent == 0 and stripped == "jobs:":
            in_jobs = True
            current_job = None
            continue

        if not in_jobs:
            continue

        if indent == 0:
            in_jobs = False
            current_job = None
            continue

        if indent == 2 and stripped.endswith(":"):
            current_job = stripped[:-1]
            jobs[current_job] = current_job
            continue

        if current_job and indent == 4 and stripped.startswith("name:"):
            jobs[current_job] = parse_scalar(stripped.split(":", 1)[1])

    if not workflow_name:
        raise ValidationError(f"{path}: missing top-level workflow name")

    return WorkflowSpec(path=path, name=workflow_name, pr_capable=pr_capable, jobs=jobs)


def load_workflows(repo_root: Path) -> dict[str, WorkflowSpec]:
    workflow_dir = repo_root / ".github" / "workflows"
    specs: dict[str, WorkflowSpec] = {}
    for path in sorted(workflow_dir.glob("*.yml")):
        spec = parse_workflow(path)
        if spec.name in specs:
            raise ValidationError(
                f"duplicate workflow name {spec.name!r}: {specs[spec.name].path} and {path}"
            )
        specs[spec.name] = spec
    if not specs:
        raise ValidationError(f"no workflows found under {workflow_dir}")
    return specs


def require_bool(parent: dict[str, Any], key: str) -> bool:
    value = parent.get(key)
    if not isinstance(value, bool):
        raise ValidationError(f"expected {key!r} to be a boolean")
    return value


def require_string(parent: dict[str, Any], key: str) -> str:
    value = parent.get(key)
    if not isinstance(value, str) or not value:
        raise ValidationError(f"expected {key!r} to be a non-empty string")
    return value


def require_string_list(parent: dict[str, Any], key: str) -> list[str]:
    value = parent.get(key)
    if not isinstance(value, list) or any(not isinstance(item, str) or not item for item in value):
        raise ValidationError(f"expected {key!r} to be a non-empty string list")
    return value


def validate_contract_schema(contract: dict[str, Any]) -> None:
    if contract.get("schema_version") != 1:
        raise ValidationError("schema_version must equal 1")

    policy = contract.get("policy")
    if not isinstance(policy, dict):
        raise ValidationError("policy must be an object")
    require_string(policy, "baseline_policy")
    require_bool(policy, "require_signed_commits")

    branch_protection = policy.get("branch_protection")
    if not isinstance(branch_protection, dict):
        raise ValidationError("policy.branch_protection must be an object")

    require_string_list(branch_protection, "protected_branches")
    require_bool(branch_protection, "require_pull_request")
    require_bool(branch_protection, "disallow_direct_pushes")
    require_bool(branch_protection, "require_up_to_date_branches")
    require_bool(branch_protection, "enforce_admins")
    require_bool(branch_protection, "allow_force_pushes")
    require_bool(branch_protection, "allow_deletions")

    required_checks = require_string_list(contract, "required_checks")
    informational_checks = require_string_list(contract, "informational_checks")

    overlap = sorted(set(required_checks) & set(informational_checks))
    if overlap:
        raise ValidationError(
            "required_checks and informational_checks overlap: " + ", ".join(overlap)
        )

    for key in ("required_checks", "informational_checks"):
        values = contract[key]
        duplicates = sorted({item for item in values if values.count(item) > 1})
        if duplicates:
            raise ValidationError(f"{key} contains duplicates: {', '.join(duplicates)}")

    waivers = contract.get("waivers")
    if not isinstance(waivers, list):
        raise ValidationError("waivers must be a list")

    expected_pr_workflows = contract.get("expected_pr_workflows")
    if not isinstance(expected_pr_workflows, list) or not expected_pr_workflows:
        raise ValidationError("expected_pr_workflows must be a non-empty list")

    for item in expected_pr_workflows:
        if not isinstance(item, dict):
            raise ValidationError("expected_pr_workflows entries must be objects")
        require_string(item, "workflow")
        require_string_list(item, "jobs")


def validate_contract_against_workflows(
    contract: dict[str, Any], workflows: dict[str, WorkflowSpec]
) -> None:
    declared_checks = set(contract["required_checks"]) | set(contract["informational_checks"])
    mapped_checks: set[str] = set()

    for item in contract["expected_pr_workflows"]:
        workflow_name = item["workflow"]
        if workflow_name not in workflows:
            raise ValidationError(f"expected PR workflow {workflow_name!r} does not exist")

        workflow = workflows[workflow_name]
        if not workflow.pr_capable:
            raise ValidationError(
                f"workflow {workflow_name!r} is declared as PR workflow but has no pull_request trigger"
            )

        for job_name in item["jobs"]:
            if job_name not in workflow.jobs.values():
                raise ValidationError(
                    f"workflow {workflow_name!r} does not define a job named {job_name!r}"
                )
            if job_name not in declared_checks:
                raise ValidationError(
                    f"workflow-backed check {job_name!r} must be declared in required_checks or informational_checks"
                )
            mapped_checks.add(job_name)

    missing = sorted(declared_checks - mapped_checks)
    if missing:
        raise ValidationError(
            "declared checks missing from expected_pr_workflows: " + ", ".join(missing)
        )


def build_branch_protection_payload(contract: dict[str, Any]) -> dict[str, Any]:
    branch_protection = contract["policy"]["branch_protection"]
    payload: dict[str, Any] = {
        "required_status_checks": {
            "strict": branch_protection["require_up_to_date_branches"],
            "contexts": contract["required_checks"],
        },
        "enforce_admins": branch_protection["enforce_admins"],
        "required_pull_request_reviews": None,
        "restrictions": None,
        "required_linear_history": False,
        "allow_force_pushes": branch_protection["allow_force_pushes"],
        "allow_deletions": branch_protection["allow_deletions"],
        "block_creations": False,
        "required_conversation_resolution": False,
        "lock_branch": False,
        "allow_fork_syncing": False,
    }
    if branch_protection["require_pull_request"]:
        payload["required_pull_request_reviews"] = {
            "dismiss_stale_reviews": False,
            "require_code_owner_reviews": False,
            "required_approving_review_count": 0,
            "require_last_push_approval": False,
        }
    return payload


def run_gh_api(endpoint: str) -> dict[str, Any]:
    result = subprocess.run(
        ["gh", "api", endpoint],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        stderr = result.stderr.strip() or result.stdout.strip() or f"exit {result.returncode}"
        raise ValidationError(f"gh api {endpoint!r} failed: {stderr}")
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise ValidationError(
            f"gh api {endpoint!r} returned invalid JSON: {exc.msg}"
        ) from exc


def run_gh_graphql(query: str) -> dict[str, Any]:
    result = subprocess.run(
        ["gh", "api", "graphql", "-f", f"query={query}"],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        stderr = result.stderr.strip() or result.stdout.strip() or f"exit {result.returncode}"
        raise ValidationError(f"gh api graphql failed: {stderr}")
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise ValidationError(f"gh api graphql returned invalid JSON: {exc.msg}") from exc


def extract_contexts(required_status_checks: dict[str, Any]) -> list[str]:
    contexts = required_status_checks.get("contexts")
    if isinstance(contexts, list):
        return [item for item in contexts if isinstance(item, str)]
    checks = required_status_checks.get("checks")
    if isinstance(checks, list):
        output: list[str] = []
        for item in checks:
            if isinstance(item, dict):
                context = item.get("context")
                if isinstance(context, str):
                    output.append(context)
        return output
    return []


def audit_github_state(contract: dict[str, Any], github_repo: str, github_branch: str) -> None:
    protected_branches = contract["policy"]["branch_protection"]["protected_branches"]
    if github_branch not in protected_branches:
        raise ValidationError(
            f"branch {github_branch!r} is not declared in protected_branches: {', '.join(protected_branches)}"
        )

    owner, repo = github_repo.split("/", 1)
    protection = run_gh_api(f"repos/{github_repo}/branches/{github_branch}/protection")
    required_signatures = run_gh_api(
        f"repos/{github_repo}/branches/{github_branch}/protection/required_signatures"
    )
    protection_rules = run_gh_graphql(
        "query { "
        f'repository(owner: "{owner}", name: "{repo}") {{ '
        "branchProtectionRules(first: 100) { "
        "nodes { "
        "pattern "
        "bypassPullRequestAllowances(first: 100) { "
        "nodes { "
        "actor { "
        "__typename "
        "... on User { login } "
        "... on App { slug } "
        "... on Team { slug organization { login } } "
        "} "
        "} "
        "} "
        "} "
        "} "
        "} "
        "}"
    )
    branch_protection = contract["policy"]["branch_protection"]
    required_status_checks = protection.get("required_status_checks") or {}
    actual_contexts = sorted(extract_contexts(required_status_checks))
    expected_contexts = sorted(contract["required_checks"])

    if actual_contexts != expected_contexts:
        raise ValidationError(
            "GitHub required status checks drift: expected "
            + ", ".join(expected_contexts)
            + " but found "
            + ", ".join(actual_contexts)
        )

    strict = required_status_checks.get("strict")
    if strict != branch_protection["require_up_to_date_branches"]:
        raise ValidationError(
            "GitHub strict status checks drift: expected "
            f"{branch_protection['require_up_to_date_branches']} but found {strict}"
        )

    enforce_admins = (protection.get("enforce_admins") or {}).get("enabled")
    if enforce_admins != branch_protection["enforce_admins"]:
        raise ValidationError(
            "GitHub enforce_admins drift: expected "
            f"{branch_protection['enforce_admins']} but found {enforce_admins}"
        )

    allow_force_pushes = (protection.get("allow_force_pushes") or {}).get("enabled")
    if allow_force_pushes != branch_protection["allow_force_pushes"]:
        raise ValidationError(
            "GitHub allow_force_pushes drift: expected "
            f"{branch_protection['allow_force_pushes']} but found {allow_force_pushes}"
        )

    allow_deletions = (protection.get("allow_deletions") or {}).get("enabled")
    if allow_deletions != branch_protection["allow_deletions"]:
        raise ValidationError(
            "GitHub allow_deletions drift: expected "
            f"{branch_protection['allow_deletions']} but found {allow_deletions}"
        )

    has_pr_gate = protection.get("required_pull_request_reviews") is not None
    if has_pr_gate != branch_protection["require_pull_request"]:
        raise ValidationError(
            "GitHub pull-request gate drift: expected "
            f"{branch_protection['require_pull_request']} but found {has_pr_gate}"
        )

    if branch_protection["require_pull_request"] and branch_protection["disallow_direct_pushes"]:
        rules = (
            protection_rules.get("data", {})
            .get("repository", {})
            .get("branchProtectionRules", {})
            .get("nodes", [])
        )
        matching_rule = next(
            (rule for rule in rules if isinstance(rule, dict) and rule.get("pattern") == github_branch),
            None,
        )
        if matching_rule is None:
            raise ValidationError(
                f"GitHub branch protection rule for {github_branch!r} is missing from GraphQL audit response"
            )
        bypass_nodes = (
            matching_rule.get("bypassPullRequestAllowances", {}).get("nodes", [])
            if isinstance(matching_rule.get("bypassPullRequestAllowances"), dict)
            else []
        )
        if bypass_nodes:
            actor_tokens: list[str] = []
            for node in bypass_nodes:
                actor = node.get("actor") if isinstance(node, dict) else None
                if not isinstance(actor, dict):
                    actor_tokens.append("<unknown-actor>")
                    continue
                actor_type = actor.get("__typename")
                if actor_type == "User":
                    actor_tokens.append(f"user:{actor.get('login', '<unknown>')}")
                elif actor_type == "App":
                    actor_tokens.append(f"app:{actor.get('slug', '<unknown>')}")
                elif actor_type == "Team":
                    org = (actor.get("organization") or {}).get("login", "<unknown-org>")
                    actor_tokens.append(f"team:{org}/{actor.get('slug', '<unknown-team>')}")
                else:
                    actor_tokens.append(f"{actor_type or 'unknown'}:<unknown>")
            raise ValidationError(
                "GitHub bypassPullRequestAllowances drift: expected none but found "
                + ", ".join(actor_tokens)
            )

    signatures_enabled = required_signatures.get("enabled")
    if signatures_enabled != contract["policy"]["require_signed_commits"]:
        raise ValidationError(
            "GitHub required_signatures drift: expected "
            f"{contract['policy']['require_signed_commits']} but found {signatures_enabled}"
        )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate the repo-local quality gates contract and optionally audit GitHub branch protection."
    )
    parser.add_argument(
        "--repo-root",
        default=".",
        help="Repository root (defaults to current directory).",
    )
    parser.add_argument(
        "--contract",
        default=".github/quality-gates.json",
        help="Path to the quality-gates contract JSON relative to repo root.",
    )
    parser.add_argument(
        "--github-live",
        metavar="OWNER/REPO",
        help="Audit live GitHub branch protection for the given repository via gh CLI.",
    )
    parser.add_argument(
        "--github-branch",
        default="main",
        help="Branch to audit when --github-live is set (default: main).",
    )
    parser.add_argument(
        "--emit-branch-protection-payload",
        action="store_true",
        help="Print the GitHub branch-protection payload derived from the contract and exit.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    repo_root = Path(args.repo_root).resolve()
    contract_path = repo_root / args.contract

    try:
        contract = load_contract(contract_path)
        validate_contract_schema(contract)
        workflows = load_workflows(repo_root)
        validate_contract_against_workflows(contract, workflows)

        if args.emit_branch_protection_payload:
            print(json.dumps(build_branch_protection_payload(contract), indent=2))
            return 0

        if args.github_live:
            audit_github_state(contract, args.github_live, args.github_branch)
    except ValidationError as exc:
        print(f"[quality-gates] {exc}", file=sys.stderr)
        return 1

    if args.github_live:
        print(
            f"[quality-gates] local contract and GitHub audit passed for {args.github_live}@{args.github_branch}"
        )
    else:
        print("[quality-gates] local contract validation passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
