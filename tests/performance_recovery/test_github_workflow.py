#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github" / "workflows" / "performance-recovery.yml"
RUNNER = ROOT / "scripts" / "run-performance-recovery-github-hosted.sh"


class GithubPerformanceRecoveryWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.source = WORKFLOW.read_text(encoding="utf-8")

    def test_workflow_is_manual_only_and_serialized(self) -> None:
        self.assertIn("  workflow_dispatch:", self.source)
        for event in ("  push:", "  pull_request:", "  schedule:"):
            self.assertNotIn(event, self.source)
        self.assertIn("group: performance-recovery-manual", self.source)
        self.assertIn("cancel-in-progress: false", self.source)
        self.assertIn("runs-on: ubuntu-24.04", self.source)
        self.assertNotIn("self-hosted", self.source)
        self.assertNotIn("192.168.31.11", self.source)
        self.assertNotIn("codex-testbox", self.source)

    def test_workflow_requires_fixture_duration_and_confirmation(self) -> None:
        self.assertIn("name: Performance Recovery A/B", self.source)
        self.assertIn("confirm:", self.source)
        self.assertIn('          - "no"', self.source)
        self.assertIn('          - "yes"', self.source)
        self.assertIn("default: no", self.source)
        self.assertIn('if [[ "${CONFIRM}" != "yes" ]]', self.source)
        self.assertIn("DURATION_SECS < 600", self.source)
        self.assertIn("DURATION_SECS > 1800", self.source)
        self.assertIn("BASELINE_REF", self.source)
        self.assertIn("candidate_ref || github.sha", self.source)

    def test_workflow_uses_the_github_hosted_fixture_comparison(self) -> None:
        self.assertIn(
            "scripts/run-performance-recovery-github-hosted.sh",
            self.source,
        )
        self.assertIn("actions/upload-artifact@v7", self.source)
        self.assertIn("retention-days: 14", self.source)
        self.assertIn(
            "Databases are generated local fixtures; no production snapshot or private network host is used.",
            self.source,
        )
        self.assertNotIn("https://api.tavily.com", self.source)
        self.assertNotIn("TAVILY_UPSTREAM", self.source)

    def test_github_hosted_runner_has_no_private_network_requirements(self) -> None:
        runner = RUNNER.read_text(encoding="utf-8")
        self.assertIn("PERFORMANCE_RECOVERY_RUN_MODE=github-hosted", runner)
        self.assertIn("docker compose", runner)
        self.assertIn('git -C "$ROOT_DIR" archive', runner)
        self.assertIn("tavily_proxy-observability.db", runner)
        self.assertNotIn("self-hosted", runner)
        self.assertNotIn("192.168.31.11", runner)
        self.assertNotIn("codex-testbox", runner)


if __name__ == "__main__":
    unittest.main()
