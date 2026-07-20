#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock


def load_module():
    script_path = Path(__file__).resolve().parents[1] / ".github" / "scripts" / "release_failure_notifier.py"
    spec = importlib.util.spec_from_file_location("release_failure_notifier", script_path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


MODULE = load_module()


class ReleaseFailureNotifierTests(unittest.TestCase):
    class FakeApi:
        def __init__(
            self,
            *,
            jobs_by_path: dict[str, list[dict]] | None = None,
            logs_by_path: dict[str, str] | None = None,
            json_by_path: dict[str, object] | None = None,
        ) -> None:
            self.jobs_by_path = jobs_by_path or {}
            self.logs_by_path = logs_by_path or {}
            self.json_by_path = json_by_path or {}
            self.rerun_calls: list[tuple[str, dict | None]] = []

        def request_json(self, path: str, *, method: str = "GET", payload: dict | None = None) -> object:
            self.rerun_calls.append((f"json:{method}:{path}", payload))
            if path in self.json_by_path:
                return self.json_by_path[path]
            return {"jobs": self.jobs_by_path[path]}

        def request_text(
            self,
            path: str,
            *,
            accept: str = MODULE.API_ACCEPT,
            method: str = "GET",
            payload: dict | None = None,
        ) -> str:
            if method == "POST":
                self.rerun_calls.append((path, payload))
                return ""
            self.rerun_calls.append((f"text:{method}:{path}", payload))
            return self.logs_by_path[path]

    def make_job(self, *, name: str, log: str):
        return MODULE.JobLog(job_id=1, name=name, conclusion="failure", log=log)

    def parse_outputs(self, path: Path) -> dict[str, str]:
        outputs: dict[str, str] = {}
        lines = path.read_text(encoding="utf-8").splitlines()
        index = 0
        while index < len(lines):
            line = lines[index]
            if "<<EOF" in line:
                key = line.split("<<EOF", 1)[0]
                index += 1
                chunks: list[str] = []
                while index < len(lines) and lines[index] != "EOF":
                    chunks.append(lines[index])
                    index += 1
                outputs[key] = "\n".join(chunks)
            elif "=" in line:
                key, value = line.split("=", 1)
                outputs[key] = value
            index += 1
        return outputs

    def test_transient_docker_failures_are_classified_for_build_jobs(self):
        jobs = [
            self.make_job(
                name="Build and smoke image (arm64)",
                log=(
                    'error: Head "https://registry-1.docker.io/v2/moby/buildkit/manifests/buildx-stable-1": '
                    'Get "https://auth.docker.io/token?...": net/http: request canceled '
                    "(Client.Timeout exceeded while awaiting headers)"
                ),
            ),
            self.make_job(
                name="Build and smoke image (amd64)",
                log=(
                    "failed to solve: failed to fetch oauth token: unexpected status from POST request to "
                    "https://auth.docker.io/token: 504 Gateway Timeout"
                ),
            ),
        ]

        result = MODULE.classify_failed_jobs(jobs)

        self.assertTrue(result.transient_docker_failure)
        self.assertIn("transient Docker failure matched", result.reason)

    def test_non_docker_jobs_are_not_auto_rerun_candidates(self):
        jobs = [
            self.make_job(
                name="GitHub Release",
                log="gh release upload failed because the asset already exists",
            )
        ]

        result = MODULE.classify_failed_jobs(jobs)

        self.assertFalse(result.transient_docker_failure)
        self.assertIn("outside Docker release scope", result.reason)

    def test_non_transient_docker_failures_do_not_match(self):
        jobs = [
            self.make_job(
                name="Build and smoke image (amd64)",
                log=(
                    "failed to solve: process \"/bin/sh -c cargo build --release --locked\" "
                    "did not complete successfully: exit code: 101"
                ),
            )
        ]

        result = MODULE.classify_failed_jobs(jobs)

        self.assertFalse(result.transient_docker_failure)
        self.assertIn("no transient Docker signature", result.reason)

    def test_release_intent_labels_recognize_patch_release(self):
        decision = MODULE.classify_release_intent_labels(
            [{"name": "type:patch"}, {"name": "channel:stable"}],
            pr_number="454",
            pr_url="https://example.test/pr/454",
        )

        self.assertTrue(decision.should_release)
        self.assertEqual(decision.reason, "intent_release")
        self.assertEqual(decision.release_intent_label, "type:patch")
        self.assertEqual(decision.release_channel, "stable")

    def test_post_rerun_failures_keep_self_heal_context_in_alert_details(self):
        base = "resolved release target sha from Prepare logs"
        current = MODULE.FailureClassification(
            transient_docker_failure=False,
            reason="current attempt failed in GitHub Release",
            failed_job_names=("GitHub Release",),
        )
        first_attempt = MODULE.FailureClassification(
            transient_docker_failure=True,
            reason="transient Docker failure matched: Build and smoke image (amd64) [auth-docker-io,gateway-timeout]",
            failed_job_names=("Build and smoke image (amd64)",),
        )

        details = MODULE.compose_extra_details(
            base,
            run_attempt=2,
            current_classification=current,
            first_attempt_classification=first_attempt,
            rerun_triggered=False,
            rerun_error="",
        )

        self.assertIn("automatic failed-jobs rerun", details)
        self.assertIn("attempt", details)

    def test_main_triggers_rerun_and_suppresses_first_transient_attempt(self):
        fake_api = self.FakeApi(
            jobs_by_path={
                "/repos/test/repo/actions/runs/42/attempts/1/jobs?per_page=100": [
                    {"id": 1, "name": "Build and smoke image (amd64)", "conclusion": "failure"}
                ]
            },
            logs_by_path={
                "/repos/test/repo/actions/jobs/1/logs": (
                    "RELEASE_TARGET_SHA=0123456789abcdef0123456789abcdef01234567\n"
                    "failed to solve: failed to fetch oauth token: unexpected status from POST request to "
                    "https://auth.docker.io/token: 504 Gateway Timeout"
                )
            },
        )

        with tempfile.TemporaryDirectory() as tmpdir:
            output_path = Path(tmpdir) / "github_output.txt"
            summary_path = Path(tmpdir) / "github_summary.txt"
            env = {
                "GH_TOKEN": "test-token",
                "REPOSITORY": "test/repo",
                "RUN_ID": "42",
                "RUN_ATTEMPT": "1",
                "RUN_EVENT": "push",
                "HEAD_BRANCH": "main",
                "HEAD_SHA": "fedcba9876543210fedcba9876543210fedcba98",
                "TRIGGERING_ACTOR": "koha",
                "GITHUB_OUTPUT": str(output_path),
                "GITHUB_STEP_SUMMARY": str(summary_path),
            }

            with mock.patch.object(MODULE, "GitHubApi", return_value=fake_api):
                with mock.patch.dict(os.environ, env, clear=False):
                    result = MODULE.main()
            outputs = self.parse_outputs(output_path)

        self.assertEqual(result, 0)
        self.assertEqual(outputs["head_sha"], "0123456789abcdef0123456789abcdef01234567")
        self.assertEqual(outputs["rerun_eligible"], "true")
        self.assertEqual(outputs["rerun_triggered"], "true")
        self.assertEqual(outputs["alert_suppressed"], "true")
        self.assertIn("automatic failed-jobs rerun triggered once", outputs["extra_details"])
        self.assertIn(("/repos/test/repo/actions/runs/42/rerun-failed-jobs", {"enable_debug_logging": False}), fake_api.rerun_calls)

    def test_main_keeps_alert_enabled_after_second_attempt(self):
        fake_api = self.FakeApi(
            jobs_by_path={
                "/repos/test/repo/actions/runs/42/attempts/2/jobs?per_page=100": [
                    {"id": 2, "name": "GitHub Release", "conclusion": "failure"}
                ],
                "/repos/test/repo/actions/runs/42/attempts/1/jobs?per_page=100": [
                    {"id": 1, "name": "Build and smoke image (amd64)", "conclusion": "failure"}
                ],
            },
            logs_by_path={
                "/repos/test/repo/actions/jobs/1/logs": (
                    "failed to solve: failed to fetch oauth token: unexpected status from POST request to "
                    "https://auth.docker.io/token: 504 Gateway Timeout"
                ),
                "/repos/test/repo/actions/jobs/2/logs": (
                    "RELEASE_TARGET_SHA=89abcdef0123456789abcdef0123456789abcdef\n"
                    "gh release upload failed because the asset already exists"
                ),
            },
        )

        with tempfile.TemporaryDirectory() as tmpdir:
            output_path = Path(tmpdir) / "github_output.txt"
            summary_path = Path(tmpdir) / "github_summary.txt"
            env = {
                "GH_TOKEN": "test-token",
                "REPOSITORY": "test/repo",
                "RUN_ID": "42",
                "RUN_ATTEMPT": "2",
                "RUN_EVENT": "push",
                "HEAD_BRANCH": "main",
                "HEAD_SHA": "fedcba9876543210fedcba9876543210fedcba98",
                "TRIGGERING_ACTOR": "koha",
                "GITHUB_OUTPUT": str(output_path),
                "GITHUB_STEP_SUMMARY": str(summary_path),
            }

            with mock.patch.object(MODULE, "GitHubApi", return_value=fake_api):
                with mock.patch.dict(os.environ, env, clear=False):
                    result = MODULE.main()
            outputs = self.parse_outputs(output_path)

        self.assertEqual(result, 0)
        self.assertEqual(outputs["rerun_eligible"], "false")
        self.assertEqual(outputs["rerun_triggered"], "false")
        self.assertEqual(outputs["alert_suppressed"], "false")
        self.assertEqual(outputs["self_heal_attempted"], "true")
        self.assertIn("automatic failed-jobs rerun", outputs["extra_details"])
        self.assertFalse(any(path == "/repos/test/repo/actions/runs/42/rerun-failed-jobs" for path, _ in fake_api.rerun_calls))

    def test_main_ci_pipeline_failure_with_release_intent_emits_alert(self):
        fake_api = self.FakeApi(
            jobs_by_path={
                "/repos/test/repo/actions/runs/42/attempts/1/jobs?per_page=100": [
                    {"id": 9, "name": "Backend Bin Tests (Bin Admin API)", "conclusion": "failure"},
                    {"id": 10, "name": "Backend Tests", "conclusion": "failure"},
                ]
            },
            json_by_path={
                "/repos/test/repo/commits/fedcba9876543210fedcba9876543210fedcba98/pulls?per_page=100": [
                    {"number": 454, "html_url": "https://example.test/pr/454"}
                ],
                "/repos/test/repo/issues/454/labels?per_page=100": [
                    {"name": "type:patch"},
                    {"name": "channel:stable"},
                ],
            },
        )

        with tempfile.TemporaryDirectory() as tmpdir:
            output_path = Path(tmpdir) / "github_output.txt"
            summary_path = Path(tmpdir) / "github_summary.txt"
            env = {
                "GH_TOKEN": "test-token",
                "REPOSITORY": "test/repo",
                "WORKFLOW_NAME": "CI Pipeline",
                "RUN_ID": "42",
                "RUN_ATTEMPT": "1",
                "RUN_EVENT": "push",
                "HEAD_BRANCH": "main",
                "HEAD_SHA": "fedcba9876543210fedcba9876543210fedcba98",
                "TRIGGERING_ACTOR": "koha",
                "GITHUB_OUTPUT": str(output_path),
                "GITHUB_STEP_SUMMARY": str(summary_path),
            }

            with mock.patch.object(MODULE, "GitHubApi", return_value=fake_api):
                with mock.patch.dict(os.environ, env, clear=False):
                    result = MODULE.main()
            outputs = self.parse_outputs(output_path)

        self.assertEqual(result, 0)
        self.assertEqual(outputs["workflow_name"], "CI Pipeline")
        self.assertEqual(outputs["release_intent_label"], "type:patch")
        self.assertEqual(outputs["release_channel"], "stable")
        self.assertEqual(outputs["alert_suppressed"], "false")
        self.assertIn("main CI failed before release started", outputs["extra_details"])
        self.assertIn("Backend Tests", outputs["extra_details"])
        self.assertIn("type:patch", outputs["extra_details"])
        self.assertFalse(any(path == "/repos/test/repo/actions/runs/42/rerun-failed-jobs" for path, _ in fake_api.rerun_calls))

    def test_main_ci_pipeline_failure_without_release_intent_suppresses_alert(self):
        fake_api = self.FakeApi(
            jobs_by_path={
                "/repos/test/repo/actions/runs/42/attempts/1/jobs?per_page=100": [
                    {"id": 9, "name": "Backend Tests", "conclusion": "failure"}
                ]
            },
            json_by_path={
                "/repos/test/repo/commits/fedcba9876543210fedcba9876543210fedcba98/pulls?per_page=100": [
                    {"number": 455, "html_url": "https://example.test/pr/455"}
                ],
                "/repos/test/repo/issues/455/labels?per_page=100": [
                    {"name": "type:skip"},
                    {"name": "channel:stable"},
                ],
            },
        )

        with tempfile.TemporaryDirectory() as tmpdir:
            output_path = Path(tmpdir) / "github_output.txt"
            summary_path = Path(tmpdir) / "github_summary.txt"
            env = {
                "GH_TOKEN": "test-token",
                "REPOSITORY": "test/repo",
                "WORKFLOW_NAME": "CI Pipeline",
                "RUN_ID": "42",
                "RUN_ATTEMPT": "1",
                "RUN_EVENT": "push",
                "HEAD_BRANCH": "main",
                "HEAD_SHA": "fedcba9876543210fedcba9876543210fedcba98",
                "TRIGGERING_ACTOR": "koha",
                "GITHUB_OUTPUT": str(output_path),
                "GITHUB_STEP_SUMMARY": str(summary_path),
            }

            with mock.patch.object(MODULE, "GitHubApi", return_value=fake_api):
                with mock.patch.dict(os.environ, env, clear=False):
                    result = MODULE.main()
            outputs = self.parse_outputs(output_path)

        self.assertEqual(result, 0)
        self.assertEqual(outputs["release_intent_label"], "type:skip")
        self.assertEqual(outputs["release_channel"], "stable")
        self.assertEqual(outputs["alert_suppressed"], "true")
        self.assertIn("suppressed", outputs["extra_details"])


if __name__ == "__main__":
    unittest.main()
