#!/usr/bin/env python3
from __future__ import annotations

import json
import os
import re
from dataclasses import dataclass
from typing import Sequence
from urllib import error, request

API_ACCEPT = "application/vnd.github+json"
CI_PIPELINE_WORKFLOW_NAME = "CI Pipeline"
RELEASE_WORKFLOW_NAME = "Release"
SHA_RE = re.compile(r"\b[0-9a-f]{40}\b")
ACTUAL_PATTERNS = [re.compile(r"\bRELEASE_TARGET_SHA=([0-9a-f]{40})\b")]
REQUESTED_PATTERNS = [re.compile(r"\bRELEASE_REQUESTED_SHA=([0-9a-f]{40})\b")]
DOCKER_JOB_PATTERNS = [
    re.compile(r"^Build and smoke image \("),
    re.compile(r"^Publish multi-arch manifest \(ghcr\)$"),
]
DOCKER_CONTEXT_PATTERNS = [
    ("auth-docker-io", re.compile(r"auth\.docker\.io", re.IGNORECASE)),
    ("registry-1-docker-io", re.compile(r"registry-1\.docker\.io", re.IGNORECASE)),
    ("moby-buildkit", re.compile(r"moby/buildkit", re.IGNORECASE)),
    ("buildx", re.compile(r"\bbuildx\b", re.IGNORECASE)),
    ("docker-oauth-token", re.compile(r"failed to fetch oauth token", re.IGNORECASE)),
]
TRANSIENT_PATTERNS = [
    ("await-headers-timeout", re.compile(r"Client\.Timeout exceeded while awaiting headers")),
    ("gateway-timeout", re.compile(r"\b504 Gateway Timeout\b")),
    ("tls-timeout", re.compile(r"TLS handshake timeout", re.IGNORECASE)),
    ("io-timeout", re.compile(r"\bi/o timeout\b", re.IGNORECASE)),
    ("request-canceled", re.compile(r"request canceled", re.IGNORECASE)),
    ("connection-reset", re.compile(r"connection reset by peer", re.IGNORECASE)),
    ("temporary-dns", re.compile(r"temporary failure in name resolution", re.IGNORECASE)),
    ("context-deadline", re.compile(r"context deadline exceeded", re.IGNORECASE)),
]
RELEASE_INTENT_LABELS = {"type:docs", "type:skip", "type:patch", "type:minor", "type:major"}
RELEASE_CHANNEL_LABELS = {"channel:stable", "channel:rc"}
NON_RELEASING_INTENT_LABELS = {"type:docs", "type:skip"}


@dataclass(frozen=True)
class JobLog:
    job_id: int
    name: str
    conclusion: str
    log: str


@dataclass(frozen=True)
class FailureClassification:
    transient_docker_failure: bool
    reason: str
    failed_job_names: tuple[str, ...]


@dataclass(frozen=True)
class ReleaseIntentDecision:
    should_release: bool
    reason: str
    pr_number: str
    pr_url: str
    release_intent_label: str
    release_channel: str


class GitHubApi:
    def __init__(self, token: str, api_root: str) -> None:
        self.token = token
        self.api_root = api_root.rstrip("/")

    def request_text(
        self,
        path: str,
        *,
        accept: str = API_ACCEPT,
        method: str = "GET",
        payload: dict | None = None,
    ) -> str:
        url = path if path.startswith("http") else f"{self.api_root}{path}"
        data = None
        if payload is not None:
            data = json.dumps(payload).encode("utf-8")
        req = request.Request(
            url,
            data=data,
            method=method,
            headers={
                "Authorization": f"Bearer {self.token}",
                "Accept": accept,
                "Content-Type": "application/json",
                "X-GitHub-Api-Version": "2022-11-28",
                "User-Agent": "tavily-hikari-release-alert-resolver",
            },
        )
        with request.urlopen(req) as resp:
            charset = resp.headers.get_content_charset() or "utf-8"
            return resp.read().decode(charset, errors="replace")

    def request_json(self, path: str, *, method: str = "GET", payload: dict | None = None) -> object:
        return json.loads(self.request_text(path, method=method, payload=payload))


def match_sha(patterns: Sequence[re.Pattern[str]], text: str) -> str:
    for pattern in patterns:
        match = pattern.search(text)
        if match:
            candidate = match.group(1)
            if SHA_RE.fullmatch(candidate):
                return candidate
    return ""


def pattern_hits(text: str, patterns: Sequence[tuple[str, re.Pattern[str]]]) -> list[str]:
    return [label for label, pattern in patterns if pattern.search(text)]


def is_docker_job_name(name: str) -> bool:
    return any(pattern.search(name) for pattern in DOCKER_JOB_PATTERNS)


def classify_failed_jobs(failed_jobs: Sequence[JobLog]) -> FailureClassification:
    if not failed_jobs:
        return FailureClassification(
            transient_docker_failure=False,
            reason="no failed jobs found in the completed release attempt",
            failed_job_names=(),
        )

    out_of_scope = [job.name for job in failed_jobs if not is_docker_job_name(job.name)]
    if out_of_scope:
        names = ", ".join(out_of_scope)
        return FailureClassification(
            transient_docker_failure=False,
            reason=f"failed job outside Docker release scope: {names}",
            failed_job_names=tuple(job.name for job in failed_jobs),
        )

    matched: list[str] = []
    unmatched: list[str] = []
    for job in failed_jobs:
        context_hits = pattern_hits(job.log, DOCKER_CONTEXT_PATTERNS)
        transient_hits = pattern_hits(job.log, TRANSIENT_PATTERNS)
        if context_hits and transient_hits:
            hits = ",".join(sorted(set(context_hits + transient_hits)))
            matched.append(f"{job.name} [{hits}]")
        else:
            unmatched.append(job.name)

    if matched and not unmatched:
        return FailureClassification(
            transient_docker_failure=True,
            reason=f"transient Docker failure matched: {'; '.join(matched)}",
            failed_job_names=tuple(job.name for job in failed_jobs),
        )

    if matched:
        matched_names = ", ".join(entry.split(" [", 1)[0] for entry in matched)
        unmatched_names = ", ".join(unmatched)
        reason = (
            "mixed Docker failure signals: "
            f"matched transient signatures in {matched_names}, but not in {unmatched_names}"
        )
    else:
        reason = "no transient Docker signature found in failed Docker jobs"

    return FailureClassification(
        transient_docker_failure=False,
        reason=reason,
        failed_job_names=tuple(job.name for job in failed_jobs),
    )


def extract_failed_job_names(jobs: Sequence[object]) -> tuple[str, ...]:
    failed_names: list[str] = []
    for job in jobs:
        if not isinstance(job, dict):
            continue
        if str(job.get("conclusion") or "").lower() != "failure":
            continue
        failed_names.append(str(job.get("name") or "unnamed job"))
    return tuple(failed_names)


def classify_release_intent_labels(
    labels: Sequence[object],
    *,
    pr_number: str,
    pr_url: str,
) -> ReleaseIntentDecision:
    names = [label.get("name", "") for label in labels if isinstance(label, dict)]
    type_like = {name for name in names if name.startswith("type:")}
    unknown_type = sorted(type_like - RELEASE_INTENT_LABELS)
    present_intent = sorted({name for name in names if name in RELEASE_INTENT_LABELS})

    channel_like = {name for name in names if name.startswith("channel:")}
    unknown_channel = sorted(channel_like - RELEASE_CHANNEL_LABELS)
    present_channel = sorted({name for name in names if name in RELEASE_CHANNEL_LABELS})

    if unknown_channel:
        return ReleaseIntentDecision(
            should_release=False,
            reason=f"unknown_channel_label({','.join(unknown_channel)})",
            pr_number=pr_number,
            pr_url=pr_url,
            release_intent_label="",
            release_channel="",
        )

    if len(present_channel) != 1:
        reason = "missing_channel_label" if len(present_channel) == 0 else f"invalid_channel_label_count({len(present_channel)})"
        return ReleaseIntentDecision(
            should_release=False,
            reason=reason,
            pr_number=pr_number,
            pr_url=pr_url,
            release_intent_label="",
            release_channel="",
        )

    channel_label = present_channel[0]
    release_channel = "rc" if channel_label == "channel:rc" else "stable"

    if unknown_type:
        return ReleaseIntentDecision(
            should_release=False,
            reason=f"unknown_intent_label({','.join(unknown_type)})",
            pr_number=pr_number,
            pr_url=pr_url,
            release_intent_label="",
            release_channel=release_channel,
        )

    if len(present_intent) != 1:
        return ReleaseIntentDecision(
            should_release=False,
            reason=f"invalid_intent_label_count({len(present_intent)})",
            pr_number=pr_number,
            pr_url=pr_url,
            release_intent_label="",
            release_channel=release_channel,
        )

    release_intent_label = present_intent[0]
    if release_intent_label in NON_RELEASING_INTENT_LABELS:
        return ReleaseIntentDecision(
            should_release=False,
            reason="intent_skip",
            pr_number=pr_number,
            pr_url=pr_url,
            release_intent_label=release_intent_label,
            release_channel=release_channel,
        )

    return ReleaseIntentDecision(
        should_release=True,
        reason="intent_release",
        pr_number=pr_number,
        pr_url=pr_url,
        release_intent_label=release_intent_label,
        release_channel=release_channel,
    )


def resolve_release_intent(api: GitHubApi, repository: str, sha: str) -> ReleaseIntentDecision:
    try:
        pulls = api.request_json(f"/repos/{repository}/commits/{sha}/pulls?per_page=100")
    except Exception as exc:  # noqa: BLE001
        return ReleaseIntentDecision(
            should_release=False,
            reason=f"api_failure:commit_pulls({type(exc).__name__})",
            pr_number="",
            pr_url="",
            release_intent_label="",
            release_channel="",
        )

    if not isinstance(pulls, list):
        return ReleaseIntentDecision(
            should_release=False,
            reason="ambiguous_or_missing_pr(count=0)",
            pr_number="",
            pr_url="",
            release_intent_label="",
            release_channel="",
        )

    if len(pulls) != 1:
        return ReleaseIntentDecision(
            should_release=False,
            reason=f"ambiguous_or_missing_pr(count={len(pulls)})",
            pr_number="",
            pr_url="",
            release_intent_label="",
            release_channel="",
        )

    pull = pulls[0]
    if not isinstance(pull, dict) or not isinstance(pull.get("number"), int):
        return ReleaseIntentDecision(
            should_release=False,
            reason="malformed_pr_payload",
            pr_number="",
            pr_url="",
            release_intent_label="",
            release_channel="",
        )

    pr_number = str(pull["number"])
    pr_url = str(pull.get("html_url") or "")

    try:
        labels = api.request_json(f"/repos/{repository}/issues/{pr_number}/labels?per_page=100")
    except Exception as exc:  # noqa: BLE001
        return ReleaseIntentDecision(
            should_release=False,
            reason=f"api_failure:pr_labels({type(exc).__name__})",
            pr_number=pr_number,
            pr_url=pr_url,
            release_intent_label="",
            release_channel="",
        )

    if not isinstance(labels, list):
        return ReleaseIntentDecision(
            should_release=False,
            reason="malformed_labels_payload",
            pr_number=pr_number,
            pr_url=pr_url,
            release_intent_label="",
            release_channel="",
        )

    return classify_release_intent_labels(labels, pr_number=pr_number, pr_url=pr_url)


def pick_ref_label(run_event: str, head_branch: str) -> str:
    if run_event == "workflow_dispatch":
        if head_branch:
            return f"dispatch ref: {head_branch}"
        return "dispatch ref: unavailable from workflow_run payload"
    if head_branch:
        return f"branch: {head_branch}"
    return "ref: unavailable from workflow_run payload"


def join_details(*parts: str) -> str:
    cleaned = [part.strip() for part in parts if part and part.strip()]
    return "; ".join(cleaned)


def compose_extra_details(
    base_extra_details: str,
    *,
    run_attempt: int,
    current_classification: FailureClassification,
    first_attempt_classification: FailureClassification | None,
    rerun_triggered: bool,
    rerun_error: str,
) -> str:
    details = [base_extra_details]

    if run_attempt > 1 and first_attempt_classification and first_attempt_classification.transient_docker_failure:
        details.append(
            "previous attempt matched a transient Docker failure and one automatic failed-jobs rerun "
            f"was already consumed ({first_attempt_classification.reason})"
        )
    elif current_classification.transient_docker_failure:
        if rerun_triggered:
            details.append(f"automatic failed-jobs rerun triggered once ({current_classification.reason})")
        elif rerun_error:
            details.append(
                "transient Docker failure matched but the automatic failed-jobs rerun request failed "
                f"({rerun_error})"
            )
        else:
            details.append(current_classification.reason)

    return join_details(*details)


def compose_ci_pipeline_extra_details(
    release_intent: ReleaseIntentDecision,
    failed_job_names: Sequence[str],
) -> str:
    details = ["main CI failed before release started"]
    failed_jobs = ", ".join(failed_job_names)
    if failed_jobs:
        details.append(f"failed jobs: {failed_jobs}")

    if release_intent.should_release:
        pr_fragment = f"PR #{release_intent.pr_number}" if release_intent.pr_number else "PR unavailable"
        details.append(
            "release intent resolved to "
            f"{release_intent.release_intent_label} on {release_intent.release_channel} channel ({pr_fragment})"
        )
    else:
        details.append(f"release-intent gate suppressed Telegram alert ({release_intent.reason})")

    return join_details(*details)


def list_jobs(api: GitHubApi, repository: str, run_id: str, run_attempt: int | None = None) -> list[dict]:
    paths: list[str] = []
    if run_attempt:
        paths.append(f"/repos/{repository}/actions/runs/{run_id}/attempts/{run_attempt}/jobs?per_page=100")
    paths.append(f"/repos/{repository}/actions/runs/{run_id}/jobs?per_page=100")

    last_error: Exception | None = None
    for path in paths:
        try:
            payload = api.request_json(path)
        except error.HTTPError as exc:
            last_error = exc
            if exc.code == 404:
                continue
            raise
        return payload.get("jobs", [])

    if last_error is not None:
        raise last_error
    return []


def inspect_jobs(
    api: GitHubApi,
    repository: str,
    jobs: Sequence[dict],
) -> tuple[str, str, str, list[JobLog]]:
    resolved_sha = ""
    requested_sha = ""
    resolved_from = ""
    failed_jobs: list[JobLog] = []

    for job in jobs:
        if not isinstance(job, dict):
            continue
        job_id = job.get("id")
        if not isinstance(job_id, int):
            continue

        job_name = str(job.get("name") or "unnamed job")
        conclusion = str(job.get("conclusion") or "")
        log_text = api.request_text(f"/repos/{repository}/actions/jobs/{job_id}/logs")

        candidate = match_sha(ACTUAL_PATTERNS, log_text)
        if candidate and not resolved_sha:
            resolved_sha = candidate
            resolved_from = job_name

        if not requested_sha:
            requested_candidate = match_sha(REQUESTED_PATTERNS, log_text)
            if requested_candidate:
                requested_sha = requested_candidate
                if not resolved_from:
                    resolved_from = job_name

        if conclusion.lower() == "failure":
            failed_jobs.append(JobLog(job_id=job_id, name=job_name, conclusion=conclusion, log=log_text))

    return resolved_sha, requested_sha, resolved_from, failed_jobs


def resolve_base_extra_details(
    *,
    run_event: str,
    resolved_sha: str,
    fallback_head_sha: str,
    requested_sha: str,
    resolved_from: str,
    error_name: str = "",
) -> str:
    if error_name:
        if run_event == "workflow_dispatch":
            return (
                "manual release dispatch; target sha resolution fell back to workflow_run head sha "
                f"({error_name})"
            )
        return f"target sha resolution fell back to workflow_run head sha ({error_name})"

    if resolved_sha and resolved_sha != fallback_head_sha:
        return f"resolved release target sha from {resolved_from} logs"
    if requested_sha and run_event == "workflow_dispatch":
        return f"resolved requested commit sha from {resolved_from} logs"
    if run_event == "workflow_dispatch":
        return "manual release dispatch"
    return ""


def trigger_failed_jobs_rerun(api: GitHubApi, repository: str, run_id: str) -> None:
    api.request_text(
        f"/repos/{repository}/actions/runs/{run_id}/rerun-failed-jobs",
        method="POST",
        payload={"enable_debug_logging": False},
    )


def write_output(key: str, value: str, output_path: str) -> None:
    with open(output_path, "a", encoding="utf-8") as handle:
        handle.write(f"{key}={value}\n")


def write_multiline_output(key: str, value: str, output_path: str) -> None:
    with open(output_path, "a", encoding="utf-8") as handle:
        handle.write(f"{key}<<EOF\n")
        handle.write(value)
        handle.write("\nEOF\n")


def main() -> int:
    api_root = os.environ.get("GITHUB_API_URL", "https://api.github.com")
    token = os.environ["GH_TOKEN"]
    repository = os.environ["REPOSITORY"]
    workflow_name = os.environ.get("WORKFLOW_NAME", "").strip()
    run_id = os.environ["RUN_ID"]
    run_attempt_raw = os.environ.get("RUN_ATTEMPT", "").strip()
    run_event = os.environ.get("RUN_EVENT", "").strip()
    head_branch = os.environ.get("HEAD_BRANCH", "").strip()
    fallback_head_sha = os.environ.get("HEAD_SHA", "").strip()
    actor = os.environ.get("TRIGGERING_ACTOR", "").strip()
    output_path = os.environ["GITHUB_OUTPUT"]
    summary_path = os.environ.get("GITHUB_STEP_SUMMARY", "")

    run_attempt = int(run_attempt_raw or "1")
    api = GitHubApi(token=token, api_root=api_root)

    resolved_sha = fallback_head_sha
    base_extra_details = ""
    current_classification = FailureClassification(False, "current attempt was not inspected", ())
    first_attempt_classification: FailureClassification | None = None
    rerun_eligible = False
    rerun_triggered = False
    rerun_error = ""
    self_heal_attempted = False
    release_intent = ReleaseIntentDecision(
        should_release=False,
        reason="workflow_scope_not_release_intent_checked",
        pr_number="",
        pr_url="",
        release_intent_label="",
        release_channel="",
    )

    if workflow_name == CI_PIPELINE_WORKFLOW_NAME:
        try:
            current_jobs = list_jobs(api, repository, run_id, run_attempt=run_attempt)
            failed_job_names = extract_failed_job_names(current_jobs)
            release_intent = resolve_release_intent(api, repository, fallback_head_sha)
            current_classification = FailureClassification(
                transient_docker_failure=False,
                reason=f"ci_pipeline_release_gate:{release_intent.reason}",
                failed_job_names=failed_job_names,
            )
            extra_details = compose_ci_pipeline_extra_details(release_intent, failed_job_names)
            alert_suppressed = not release_intent.should_release
        except Exception as exc:  # noqa: BLE001
            current_classification = FailureClassification(
                transient_docker_failure=False,
                reason=f"ci_pipeline_release_gate:exception({type(exc).__name__})",
                failed_job_names=(),
            )
            extra_details = join_details(
                "main CI failed before release started",
                f"release-intent gate suppressed Telegram alert (exception:{type(exc).__name__})",
            )
            alert_suppressed = True
    else:
        try:
            current_jobs = list_jobs(api, repository, run_id, run_attempt=run_attempt)
            current_resolved_sha, requested_sha, resolved_from, current_failed_jobs = inspect_jobs(
                api,
                repository,
                current_jobs,
            )
            if current_resolved_sha:
                resolved_sha = current_resolved_sha
            elif requested_sha:
                resolved_sha = requested_sha

            base_extra_details = resolve_base_extra_details(
                run_event=run_event,
                resolved_sha=current_resolved_sha,
                fallback_head_sha=fallback_head_sha,
                requested_sha=requested_sha,
                resolved_from=resolved_from,
            )
            current_classification = classify_failed_jobs(current_failed_jobs)
            rerun_eligible = run_attempt == 1 and current_classification.transient_docker_failure

            if run_attempt > 1:
                first_attempt_jobs = list_jobs(api, repository, run_id, run_attempt=1)
                _, _, _, first_attempt_failed_jobs = inspect_jobs(api, repository, first_attempt_jobs)
                first_attempt_classification = classify_failed_jobs(first_attempt_failed_jobs)
                self_heal_attempted = first_attempt_classification.transient_docker_failure

            if rerun_eligible:
                try:
                    trigger_failed_jobs_rerun(api, repository, run_id)
                    rerun_triggered = True
                except Exception as exc:  # noqa: BLE001
                    rerun_error = f"{type(exc).__name__}: {exc}"
        except Exception as exc:  # noqa: BLE001
            base_extra_details = resolve_base_extra_details(
                run_event=run_event,
                resolved_sha="",
                fallback_head_sha=fallback_head_sha,
                requested_sha="",
                resolved_from="",
                error_name=type(exc).__name__,
            )
            rerun_error = rerun_error or f"{type(exc).__name__}: {exc}"

        extra_details = compose_extra_details(
            base_extra_details,
            run_attempt=run_attempt,
            current_classification=current_classification,
            first_attempt_classification=first_attempt_classification,
            rerun_triggered=rerun_triggered,
            rerun_error=rerun_error,
        )
        alert_suppressed = rerun_triggered

    write_output("ref_label", pick_ref_label(run_event, head_branch), output_path)
    write_output("head_sha", resolved_sha or fallback_head_sha, output_path)
    write_output("actor", actor, output_path)
    write_output("workflow_name", workflow_name or RELEASE_WORKFLOW_NAME, output_path)
    write_output("release_intent_label", release_intent.release_intent_label, output_path)
    write_output("release_channel", release_intent.release_channel, output_path)
    write_output("pr_number", release_intent.pr_number, output_path)
    write_multiline_output("pr_url", release_intent.pr_url, output_path)
    write_multiline_output("extra_details", extra_details, output_path)
    write_output("transient_docker_failure", str(current_classification.transient_docker_failure).lower(), output_path)
    write_output("rerun_eligible", str(rerun_eligible).lower(), output_path)
    write_output("rerun_triggered", str(rerun_triggered).lower(), output_path)
    write_output("alert_suppressed", str(alert_suppressed).lower(), output_path)
    write_output("self_heal_attempted", str(self_heal_attempted).lower(), output_path)
    write_multiline_output("rerun_reason", current_classification.reason, output_path)
    write_multiline_output("rerun_error", rerun_error, output_path)

    if summary_path:
        with open(summary_path, "a", encoding="utf-8") as handle:
            handle.write("### Release failure triage\n")
            handle.write(f"- workflow_name: {workflow_name or RELEASE_WORKFLOW_NAME}\n")
            handle.write(f"- run_attempt: {run_attempt}\n")
            if release_intent.release_intent_label or release_intent.reason != "workflow_scope_not_release_intent_checked":
                handle.write(f"- release_intent_label: {release_intent.release_intent_label or '<none>'}\n")
                handle.write(f"- release_channel: {release_intent.release_channel or '<none>'}\n")
                handle.write(f"- release_intent_reason: {release_intent.reason}\n")
                if release_intent.pr_number:
                    handle.write(f"- pr_number: {release_intent.pr_number}\n")
            handle.write(f"- transient_docker_failure: {str(current_classification.transient_docker_failure).lower()}\n")
            handle.write(f"- rerun_eligible: {str(rerun_eligible).lower()}\n")
            handle.write(f"- rerun_triggered: {str(rerun_triggered).lower()}\n")
            handle.write(f"- alert_suppressed: {str(alert_suppressed).lower()}\n")
            if current_classification.failed_job_names:
                handle.write(f"- failed_jobs: {', '.join(current_classification.failed_job_names)}\n")
            handle.write(f"- reason: {current_classification.reason}\n")
            if rerun_error:
                handle.write(f"- rerun_error: {rerun_error}\n")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
