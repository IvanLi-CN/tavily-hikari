#!/usr/bin/env python3
"""Regression tests for the mock-only local performance boundary harness."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
HARNESS_PATH = ROOT / "scripts" / "perf-local-boundaries.py"
SPEC = importlib.util.spec_from_file_location("perf_local_boundaries", HARNESS_PATH)
assert SPEC is not None and SPEC.loader is not None
HARNESS = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = HARNESS
SPEC.loader.exec_module(HARNESS)


COHORT = {"subjects": ["subject-a", "subject-b"], "tokens": ["token-a", "token-b"]}


def measured_result(
    *,
    variant: str,
    round_number: int,
    request_p95: float,
    unrelated_p95: float | None,
    request_samples: int = 30,
    unrelated_samples: int = 4,
    cohort: dict[str, list[str]] | None = None,
    status: str = "measured",
    reason: str | None = None,
) -> dict[str, object]:
    return {
        "variant": variant,
        "round": round_number,
        "ref": f"{variant}-ref",
        "status": status,
        "reason": reason,
        "duration_seconds": 600,
        "request": {
            "samples": request_samples,
            "mean_ms": request_p95 / 2,
            "p95_ms": request_p95,
            "throughput_rps": 0.05,
        },
        "unrelated_cohort": {
            "samples": unrelated_samples,
            "mean_ms": unrelated_p95,
            "p95_ms": unrelated_p95,
            "throughput_rps": 0.01,
        },
        "statuses": {"business:200": request_samples},
        "errors": {},
        "phases": {"total": {"count": request_samples, "sum_seconds": 1.0}},
        "accepted_progress": {"responses": request_samples, "reported": request_samples},
        "cohort": cohort or COHORT,
    }


def paired_round(
    round_number: int,
    baseline: dict[str, object],
    candidate: dict[str, object],
) -> dict[str, object]:
    return {"round": round_number, "baseline": baseline, "candidate": candidate}


class LocalBoundaryComparatorTests(unittest.TestCase):
    def test_single_round_regression_is_preserved_instead_of_hidden_by_aggregate_p95(self) -> None:
        rounds = [
            paired_round(
                1,
                measured_result(
                    variant="baseline", round_number=1, request_p95=100, unrelated_p95=100
                ),
                measured_result(
                    variant="candidate", round_number=1, request_p95=100, unrelated_p95=100
                ),
            ),
            paired_round(
                2,
                measured_result(
                    variant="baseline", round_number=2, request_p95=200, unrelated_p95=100
                ),
                measured_result(
                    variant="candidate", round_number=2, request_p95=250, unrelated_p95=100
                ),
            ),
            paired_round(
                3,
                measured_result(
                    variant="baseline", round_number=3, request_p95=300, unrelated_p95=100
                ),
                measured_result(
                    variant="candidate", round_number=3, request_p95=300, unrelated_p95=100
                ),
            ),
        ]

        comparison = HARNESS.compare_rounds(rounds, min_samples=30)

        self.assertEqual(
            [round_result["affected"]["p95_regression_ratio"] for round_result in comparison["rounds"]],
            [0.0, 0.25, 0.0],
        )
        self.assertTrue(comparison["summary"]["p95_is_descriptive_only"])

    def test_unstable_control_is_unvalidated_instead_of_validated(self) -> None:
        rounds = [
            paired_round(
                1,
                measured_result(
                    variant="baseline", round_number=1, request_p95=100, unrelated_p95=100
                ),
                measured_result(
                    variant="candidate", round_number=1, request_p95=100, unrelated_p95=100
                ),
            ),
            paired_round(
                2,
                measured_result(
                    variant="baseline", round_number=2, request_p95=100, unrelated_p95=100
                ),
                measured_result(
                    variant="candidate", round_number=2, request_p95=100, unrelated_p95=200
                ),
            ),
            paired_round(
                3,
                measured_result(
                    variant="baseline", round_number=3, request_p95=100, unrelated_p95=100
                ),
                measured_result(
                    variant="candidate", round_number=3, request_p95=100, unrelated_p95=100
                ),
            ),
        ]

        comparison = HARNESS.compare_rounds(rounds, min_samples=30)

        self.assertEqual(comparison["status"], "unvalidated")
        self.assertIn("round-2/control/p95_regression_over_5_percent", comparison["reasons"])

    def test_missing_measurement_and_control_report_exact_reasons(self) -> None:
        rounds = [
            paired_round(
                1,
                measured_result(
                    variant="baseline",
                    round_number=1,
                    request_p95=100,
                    unrelated_p95=None,
                    request_samples=12,
                    unrelated_samples=0,
                ),
                measured_result(
                    variant="candidate", round_number=1, request_p95=100, unrelated_p95=100
                ),
            )
        ]

        comparison = HARNESS.compare_rounds(rounds, min_samples=30)

        self.assertEqual(comparison["status"], "unvalidated")
        self.assertEqual(
            comparison["reasons"],
            [
                "round-1/baseline/request.samples_below_minimum",
                "round-1/baseline/control.samples_missing",
            ],
        )

    def test_cohort_mismatch_is_unvalidated(self) -> None:
        rounds = [
            paired_round(
                1,
                measured_result(
                    variant="baseline", round_number=1, request_p95=100, unrelated_p95=100
                ),
                measured_result(
                    variant="candidate",
                    round_number=1,
                    request_p95=100,
                    unrelated_p95=100,
                    cohort={"subjects": ["subject-z"], "tokens": ["token-z"]},
                ),
            )
        ]

        comparison = HARNESS.compare_rounds(rounds, min_samples=30)

        self.assertEqual(comparison["status"], "unvalidated")
        self.assertIn("round-1/cohort/mismatch", comparison["reasons"])


class LocalBoundaryCheckpointTests(unittest.TestCase):
    def test_service_logs_are_bounded_and_redacted(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "stderr.log"
            writer = HARNESS.BoundedLogWriter(path, max_bytes=64)
            writer.write(b"Authorization: Bearer tvly-secret-value token=raw-token\n")
            writer.write(b"x" * 200)
            writer.close()

            content = path.read_text(encoding="utf-8")

            self.assertLessEqual(path.stat().st_size, 64)
            self.assertNotIn("tvly-secret-value", content)
            self.assertNotIn("raw-token", content)

    def test_interrupted_variant_checkpoint_remains_readable_and_redacted(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            run_directory = Path(temporary) / "run"
            checkpoint = HARNESS.CheckpointStore(
                run_directory,
                {
                    "baseline_ref": "baseline-ref",
                    "candidate_ref": "candidate-ref",
                    "subjects": ["subject-a"],
                    "tokens": ["token-a"],
                },
            )
            checkpoint.write_variant_started(
                phase="initial",
                round_number=1,
                variant="baseline",
                ref="baseline-ref",
                duration_seconds=600,
            )
            checkpoint.mark_interrupted("SIGINT")

            variant_path = run_directory / "variants" / "initial-round-1-baseline.json"
            progress_path = run_directory / "progress.json"
            variant = json.loads(variant_path.read_text(encoding="utf-8"))
            progress = json.loads(progress_path.read_text(encoding="utf-8"))

            self.assertEqual(variant["status"], "interrupted")
            self.assertEqual(variant["reason"], "SIGINT")
            self.assertEqual(progress["status"], "interrupted")
            self.assertNotIn("tvly-secret-value", variant_path.read_text(encoding="utf-8"))

    def test_completed_variant_checkpoint_contains_measurement_fields(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            run_directory = Path(temporary) / "run"
            checkpoint = HARNESS.CheckpointStore(run_directory, {"kind": "test"})
            result = measured_result(
                variant="candidate", round_number=1, request_p95=120, unrelated_p95=100
            )
            checkpoint.write_variant_result("initial", 1, "candidate", result)

            variant_path = run_directory / "variants" / "initial-round-1-candidate.json"
            variant = json.loads(variant_path.read_text(encoding="utf-8"))

            self.assertEqual(variant["status"], "measured")
            self.assertEqual(variant["duration_seconds"], 600)
            for field in ("request", "unrelated_cohort", "statuses", "errors", "phases"):
                self.assertIn(field, variant)


if __name__ == "__main__":
    unittest.main()
