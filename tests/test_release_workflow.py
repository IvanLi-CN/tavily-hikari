#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
import re
import unittest


WORKFLOW_PATH = Path(__file__).resolve().parents[1] / ".github" / "workflows" / "release.yml"
DOCKERFILE_PATH = Path(__file__).resolve().parents[1] / "Dockerfile"
S2PG_SMOKE_PATH = (
    Path(__file__).resolve().parents[1] / ".github" / "scripts" / "release-s2pg-smoke.sh"
)


def job_block(source: str, job: str) -> str:
    match = re.search(
        rf"^  {re.escape(job)}:\n(?P<block>.*?)(?=^  [a-z0-9-]+:\n|\Z)",
        source,
        re.M | re.S,
    )
    assert match is not None, job
    return match.group("block")


def step_block(source: str, step: str) -> str:
    marker = f"      - name: {step}\n"
    start = source.index(marker) + len(marker)
    next_step = source.find("\n      - name: ", start)
    return source[start:] if next_step == -1 else source[start:next_step]


def dockerfile_stage(source: str, stage: str) -> str:
    marker = f" AS {stage}\n"
    start = source.index(marker) + len(marker)
    next_stage = source.find("\nFROM ", start)
    return source[start:] if next_stage == -1 else source[start:next_stage]


class ReleaseWorkflowContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.source = WORKFLOW_PATH.read_text(encoding="utf-8")
        github_release_start = cls.source.index("  github-release:\n")
        cls.github_release = cls.source[github_release_start:]

    def test_release_does_not_write_completion_comments_to_source_pr(self):
        for obsolete in (
            "Upsert PR release comment",
            "actions/github-script",
            "tavily-hikari-release-comment",
            "issues.listComments",
            "issues.updateComment",
            "issues.createComment",
            "release comment",
        ):
            self.assertNotIn(obsolete, self.github_release)

        self.assertNotIn("      issues: write\n", self.github_release)
        self.assertNotIn("      pull-requests: write\n", self.github_release)
        self.assertIn("      contents: write\n", self.github_release)

    def test_release_preparation_and_publication_contracts_remain(self):
        for preserved in (
            "      pr_number: ${{ steps.intent.outputs.pr_number }}",
            "      pr_url: ${{ steps.intent.outputs.pr_url }}",
            "            echo \"- pr: ${{ steps.intent.outputs.pr_number }} (${{ steps.intent.outputs.pr_url }})\"",
            "      release_intent_label: ${{ steps.intent.outputs.release_intent_label }}",
            "      - name: Create and push tag (if missing)",
            "  docker-manifest:",
            "  github-release:",
            "      - name: Create/Update GitHub Release",
            "      - name: Upload binary assets to GitHub Release",
            "      - name: Upload CLI assets to GitHub Release",
            "          gh release upload \"${RELEASE_TAG}\"",
        ):
            self.assertIn(preserved, self.source)

    def test_release_job_permissions_are_limited_to_publication(self):
        match = re.search(r"  github-release:\n(?P<block>.*?)(?=\n  [a-z0-9-]+:\n|\Z)", self.source, re.S)
        self.assertIsNotNone(match)
        assert match is not None
        block = match.group("block")
        self.assertIn("    permissions:\n      contents: write", block)
        self.assertNotIn("issues:", block)
        self.assertNotIn("pull-requests:", block)


class MigratorImageContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.workflow = WORKFLOW_PATH.read_text(encoding="utf-8")
        cls.dockerfile = DOCKERFILE_PATH.read_text(encoding="utf-8")
        cls.smoke = S2PG_SMOKE_PATH.read_text(encoding="utf-8")

    def test_dockerfile_builds_the_migrator_without_the_release_version(self):
        builder = dockerfile_stage(self.dockerfile, "migrator-builder")
        self.assertIn("--bin s2pg", builder)
        self.assertNotIn("APP_EFFECTIVE_VERSION", builder)

        migrate = dockerfile_stage(self.dockerfile, "migrate")
        self.assertIn(
            "from=migrator-payload-normalizer,source=/payload/bin/s2pg",
            migrate,
        )
        self.assertNotIn("APP_EFFECTIVE_VERSION", migrate)
        self.assertNotIn("org.opencontainers.image.version", migrate)
        self.assertNotIn("from=app-builder", migrate)
        self.assertNotIn("from=payload-normalizer", migrate)
        # SQLite is linked statically through libsqlite3-sys/bundled, so the migrator
        # runtime stage must not pull a SQLite client library in.
        self.assertNotIn("libsqlite3-0", migrate)
        self.assertNotIn("libsqlite3-dev", migrate)
        self.assertNotIn("pkg-config", migrate)
        self.assertIn("ca-certificates", migrate)

        stages = re.findall(r"^FROM \S+ AS (\w[\w.-]*)", self.dockerfile, re.M)
        self.assertEqual(stages[-1], "migrate")

    def test_docker_native_builds_and_smokes_both_images(self):
        block = job_block(self.workflow, "docker-native")
        self.assertIn("    services:\n      postgres:\n        image: postgres:18", block)
        self.assertIn("LOCAL_MIGRATOR_IMAGE=tavily-hikari-s2pg-release-smoke", block)
        self.assertEqual(block.count("          target: migrate\n"), 2)
        for step in (
            "Build local migrator image for s2pg smoke",
            "s2pg migrator smoke gate",
            "Build and push migrator image by digest",
            "Export migrator image digest",
            "Upload migrator image digest",
        ):
            self.assertIn(f"      - name: {step}\n", block)
        self.assertIn("./.github/scripts/release-s2pg-smoke.sh", block)
        self.assertIn(
            '".github/scripts/release-s2pg-smoke.sh"',
            block,
            "backfill helper restore list must cover the migrator smoke",
        )
        self.assertIn(
            "name: release-digest-s2pg-${{ matrix.arch }}", block
        )
        self.assertIn("name: release-digest-${{ matrix.arch }}", block)
        self.assertIn(
            "DIGEST_FILE=${RUNNER_TEMP}/release-digests/${{ matrix.arch }}.txt",
            block,
        )
        self.assertIn(
            "MIGRATOR_DIGEST_FILE=${RUNNER_TEMP}/release-digests/s2pg-${{ matrix.arch }}.txt",
            block,
        )

    def test_docker_manifest_groups_digests_per_image(self):
        block = job_block(self.workflow, "docker-manifest")
        self.assertNotIn("expected exactly 2 architecture digests", block)
        self.assertIn("expected 4 digests (2 images x 2 architectures)", block)
        self.assertIn(
            'create_manifest "server" "${REGISTRY}/${IMAGE_NAME_LOWER}" "" "${SERVER_TAGS}"',
            block,
        )
        self.assertIn(
            'create_manifest "s2pg" "${REGISTRY}/${MIGRATOR_IMAGE_LOWER}" "s2pg-" "${MIGRATOR_TAGS}"',
            block,
        )
        self.assertIn('validate_platforms "server" "${SERVER_TAGS}"', block)
        self.assertIn('validate_platforms "s2pg" "${MIGRATOR_TAGS}"', block)
        self.assertIn("for arch in amd64 arm64; do", block)

        server_meta = step_block(block, "Extract metadata (tags, labels)")
        self.assertIn("type=raw,value=latest", server_meta)
        self.assertIn("type=raw,value=v${{ env.APP_EFFECTIVE_VERSION }}", server_meta)

        migrator_meta = step_block(block, "Extract migrator metadata (version tag only)")
        self.assertIn("type=raw,value=v${{ env.APP_EFFECTIVE_VERSION }}", migrator_meta)
        self.assertNotIn("latest", migrator_meta)

    def test_migrator_smoke_keeps_the_server_smoke_coverage(self):
        self.assertIn("set -Eeuo pipefail", self.smoke)
        self.assertIn(
            "snapshot --sqlite /work/fixture/sample.db --out /work/snapshot/sample.snapshot.db",
            self.smoke,
        )
        self.assertEqual(self.smoke.count("--sqlite /work/"), 1)
        self.assertIn("snapshot --sqlite <db-path> --out <snapshot-path>", self.smoke)
        self.assertEqual(
            self.smoke.count("--snapshot /work/snapshot/sample.snapshot.db"),
            4,
        )
        self.assertIn('--database-url "${PG_URL}"', self.smoke)
        self.assertIn("--report /work/reports/verification.json", self.smoke)
        self.assertNotIn("--pg-url", self.smoke)
        self.assertIn("EXPECTED_BASELINE_TABLES", self.smoke)
        self.assertIn("core.meta", self.smoke)
        self.assertIn("core.auth_token_quota", self.smoke)
        self.assertIn("refused the non-empty target", self.smoke)
        self.assertIn(
            "UPDATE core.meta SET value = 'tampered-by-release-smoke'", self.smoke
        )
        self.assertIn("verify rejected the mutated target", self.smoke)

    def test_release_workflow_adds_no_tolerated_failure(self):
        self.assertNotIn("continue-on-error", self.workflow)
        self.assertNotIn("continue-on-error", self.smoke)


if __name__ == "__main__":
    unittest.main()
