#!/usr/bin/env python3
"""Regression tests for the mock-only local performance boundary harness."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sqlite3
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


COHORT = HARNESS.logical_cohort(
    tuple(f"subject-{index}" for index in range(1, 9)),
    tuple(f"token-{index}" for index in range(1, 9)),
    0.25,
)
SCENARIOS = HARNESS.scenario_contract(0.25)
MAPPING = {
    "status": "verified",
    "source": "test fixture",
    "credentials": [
        {
            "label": f"token-{index}",
            "token_id": f"{index:04d}",
            "billing_subject": f"token:{index:04d}",
        }
        for index in range(1, 9)
    ],
}
RESOURCES = {
    "status": "validated",
    "requested": {"raw": "cpu=2,memory=1GiB,sqlite_pool=4"},
    "startup": {"sqlite_pool_max_connections": 4},
}


def measured_result(
    *,
    variant: str,
    round_number: int,
    request_p95: float,
    unrelated_p95: float | None,
    request_samples: int = 30,
    unrelated_samples: int = 30,
    cohort: dict[str, object] | None = None,
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
        "accepted_progress": {
            "source": "maintenance_or_recovery_response_field_only",
            "responses": 0,
            "reported": 0,
        },
        "cohort": cohort or COHORT,
        "scenarios": [],
        "scenario_contract": SCENARIOS,
        "credential_mapping": MAPPING,
        "resource_observation": RESOURCES,
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

    def test_control_sample_adequacy_is_required_even_when_ratio_is_stable(self) -> None:
        rounds = [
            paired_round(
                1,
                measured_result(
                    variant="baseline",
                    round_number=1,
                    request_p95=100,
                    unrelated_p95=100,
                    unrelated_samples=3,
                ),
                measured_result(
                    variant="candidate",
                    round_number=1,
                    request_p95=100,
                    unrelated_p95=100,
                    unrelated_samples=3,
                ),
            )
        ]

        comparison = HARNESS.compare_rounds(rounds, min_samples=1, min_control_samples=30)

        self.assertEqual(comparison["status"], "unvalidated")
        self.assertIn("round-1/baseline/control.samples_below_minimum", comparison["reasons"])
        self.assertIn("round-1/candidate/control.samples_below_minimum", comparison["reasons"])

    def test_cross_round_control_variation_is_unvalidated(self) -> None:
        rounds = [
            paired_round(
                number,
                measured_result(
                    variant="baseline", round_number=number, request_p95=100, unrelated_p95=control
                ),
                measured_result(
                    variant="candidate", round_number=number, request_p95=100, unrelated_p95=100
                ),
            )
            for number, control in ((1, 100), (2, 110), (3, 100))
        ]

        comparison = HARNESS.compare_rounds(rounds, min_samples=30)

        self.assertEqual(comparison["status"], "unvalidated")
        self.assertIn(
            "rounds/control/baseline_p95_variation_over_5_percent",
            comparison["reasons"],
        )
        self.assertFalse(comparison["summary"]["control_stability"]["stable"])

    def test_single_round_control_regression_over_five_percent_is_unvalidated(self) -> None:
        rounds = [
            paired_round(
                1,
                measured_result(variant="baseline", round_number=1, request_p95=100, unrelated_p95=100),
                measured_result(variant="candidate", round_number=1, request_p95=100, unrelated_p95=106),
            )
        ]

        comparison = HARNESS.compare_rounds(rounds, min_samples=30)

        self.assertEqual(comparison["status"], "unvalidated")
        self.assertIn("round-1/control/p95_regression_over_5_percent", comparison["reasons"])

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
    def test_business_success_does_not_count_as_accepted_maintenance_progress(self) -> None:
        recorder = HARNESS.SampleRecorder()
        recorder.add_status("business", 200, 10.0, b'{"accepted_progress": 99}')
        recorder.add_status("maintenance", 202, 11.0, b'{"accepted_progress": 3}')

        progress = recorder.result(1)["accepted_progress"]

        self.assertEqual(progress["responses"], 1)
        self.assertEqual(progress["reported"], 3)

    def test_scenario_contract_and_bindings_prove_distinct_and_shared_subjects(self) -> None:
        scenarios = HARNESS.scenario_contract(0.25)
        self.assertEqual(
            [scenario["name"] for scenario in scenarios],
            [
                "serial",
                "cross_account_concurrency_4",
                "cross_account_concurrency_8",
                "same_account_queue",
            ],
        )
        credentials = tuple(
            HARNESS.Credential(
                label=f"token-{index}",
                secret=f"secret-{index}",
                token_id=f"{index:04d}",
                billing_subject=f"token:{index:04d}",
            )
            for index in range(1, 9)
        )
        cross, _ = HARNESS.scenario_bindings(
            "cross_account_concurrency_8", credentials, tuple(f"subject-{i}" for i in range(1, 9))
        )
        same, _ = HARNESS.scenario_bindings(
            "same_account_queue", credentials, tuple(f"subject-{i}" for i in range(1, 9))
        )

        self.assertEqual(len({credential.billing_subject for credential in cross}), 8)
        self.assertEqual(len({credential.billing_subject for credential in same}), 1)

    def test_resource_spec_is_parsed_into_enforceable_values(self) -> None:
        spec = HARNESS.parse_resource_spec("cpu=2,memory=1GiB,sqlite_pool=4")

        self.assertEqual(spec["cpu"], 2)
        self.assertEqual(spec["memory_bytes"], 1024**3)
        self.assertEqual(spec["sqlite_pool"], 4)

    def test_runtime_binding_check_rejects_bound_tokens(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            database = Path(temporary) / "tokens.db"
            with sqlite3.connect(database) as connection:
                connection.executescript(
                    """
                    CREATE TABLE auth_tokens (id TEXT PRIMARY KEY);
                    CREATE TABLE user_token_bindings (token_id TEXT, user_id TEXT);
                    INSERT INTO auth_tokens (id) VALUES ('0001'), ('0002');
                    """
                )
            credentials = (
                HARNESS.Credential("token-1", "secret-1", "0001", "token:0001"),
                HARNESS.Credential("token-2", "secret-2", "0002", "token:0002"),
            )

            self.assertEqual(HARNESS.verify_credential_bindings(database, credentials)["status"], "passed")
            with sqlite3.connect(database) as connection:
                connection.execute(
                    "INSERT INTO user_token_bindings (token_id, user_id) VALUES ('0002', 'user-2')"
                )
            self.assertEqual(HARNESS.verify_credential_bindings(database, credentials)["status"], "failed")

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

    def test_phase_comparison_is_persisted_before_any_extension(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            run_directory = Path(temporary) / "run"
            checkpoint = HARNESS.CheckpointStore(run_directory, {"kind": "test"})
            rounds = [{"round": 1, "baseline": {}, "candidate": {}}]
            comparison = {"status": "unvalidated", "reasons": ["round-1/control/samples_missing"]}
            checkpoint.write_phase_comparison("initial", rounds, comparison)

            phase_path = run_directory / "phases" / "initial-comparison.json"
            phase = json.loads(phase_path.read_text(encoding="utf-8"))

            self.assertEqual(phase["status"], "unvalidated")
            self.assertEqual(phase["reasons"], comparison["reasons"])
            self.assertEqual(phase["rounds"], rounds)


if __name__ == "__main__":
    unittest.main()
