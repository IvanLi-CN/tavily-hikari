#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github" / "workflows" / "performance-recovery.yml"


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
        self.assertIn("runs-on: [self-hosted, linux, x64, performance-recovery]", self.source)

    def test_workflow_requires_production_duration_and_confirmation(self) -> None:
        self.assertIn("name: Performance Recovery A/B", self.source)
        self.assertIn("confirm:", self.source)
        self.assertIn("default: no", self.source)
        self.assertIn('if [[ "${CONFIRM}" != "yes" ]]', self.source)
        self.assertIn("DURATION_SECS < 600", self.source)
        self.assertIn("DURATION_SECS > 1800", self.source)
        self.assertIn("BASELINE_REF", self.source)
        self.assertIn("candidate_ref || github.sha", self.source)

    def test_workflow_reuses_isolated_mock_comparison_and_keeps_snapshots_internal(self) -> None:
        self.assertIn(
            "scripts/run-performance-recovery-testbox-comparison.sh",
            self.source,
        )
        self.assertIn("SOURCE_SSH_TARGET: 192.168.31.11", self.source)
        self.assertIn("TESTBOX_HOST: codex-testbox", self.source)
        self.assertIn('REMOTE_SPACE_MARGIN_BYTES: "10737418240"', self.source)
        self.assertIn("actions/upload-artifact@v7", self.source)
        self.assertIn("retention-days: 14", self.source)
        self.assertIn(
            "Raw production SQLite snapshots remain on the internal runner/testbox.",
            self.source,
        )
        self.assertNotIn("https://api.tavily.com", self.source)
        self.assertNotIn("TAVILY_UPSTREAM", self.source)


if __name__ == "__main__":
    unittest.main()
