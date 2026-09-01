#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
import re
import unittest


WORKFLOW_PATH = Path(__file__).resolve().parents[1] / ".github" / "workflows" / "notify-release-failure.yml"
NOTIFIER_PATH = Path(__file__).resolve().parents[1] / ".github" / "scripts" / "release_failure_notifier.py"
OIDRUNE_NOTIFY = "IvanLi-CN/oidrune/.github/workflows/notify.yml@e48822f99c6402a753ed86557ea029754cbab20b"


class NotifyReleaseWorkflowContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.source = WORKFLOW_PATH.read_text(encoding="utf-8")
        cls.notifier_source = NOTIFIER_PATH.read_text(encoding="utf-8")

    def test_calls_trusted_oidrune_workflow_with_full_sha_only(self):
        self.assertEqual(self.source.count(f"uses: {OIDRUNE_NOTIFY}"), 2)
        self.assertNotIn("github-workflows/.github/workflows/release-failure-telegram.yml", self.source)
        self.assertNotIn("@main", self.source)
        self.assertNotIn("gateway_url", self.source)
        self.assertNotIn("oidc_audience", self.source)
        self.assertNotIn("SHOUTRRR_URL", self.source)
        self.assertNotIn("secrets:", self.source)

    def test_both_callers_grant_oidc_and_use_new_inputs(self):
        for job_name in ("notify_failure", "smoke_test"):
            start = self.source.index(f"  {job_name}:\n")
            next_job = re.search(r"\n  [a-z0-9_]+:\n", self.source[start + 4 :])
            end = len(self.source) if next_job is None else start + 4 + next_job.start()
            block = self.source[start:end]
            self.assertIn("permissions:\n      id-token: write", block)
            self.assertIn("outcome:", block)
            self.assertIn("summary:", block)

    def test_filters_and_smoke_path_remain_scoped(self):
        self.assertIn("      - Release\n      - CI Pipeline", self.source)
        self.assertIn("    types:\n      - completed", self.source)
        self.assertIn("    branches:\n      - main", self.source)
        self.assertIn("  workflow_dispatch:", self.source)
        self.assertIn("github.event.workflow_run.conclusion == 'failure'", self.source)
        self.assertIn("github.event_name == 'workflow_dispatch'", self.source)
        self.assertIn("outcome: failure", self.source)

    def test_summary_is_complete_for_failure_and_smoke_notifications(self):
        for field in ("project:", "status:", "result:", "target_sha:", "run_url:"):
            self.assertIn(field, self.source)
        self.assertIn("smoke_title:", self.source)
        self.assertIn("failure_title:", self.notifier_source)
        self.assertIn("summary: ${{ needs.resolve_release_context.outputs.summary }}", self.source)
        self.assertIn("summary: ${{ steps.resolve.outputs.summary }}", self.source)


if __name__ == "__main__":
    unittest.main()
