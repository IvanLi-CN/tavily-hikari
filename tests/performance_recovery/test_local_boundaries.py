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
    "scope": "launcher_process_and_inherited_descendants",
    "requested": {
        "raw": "cpu=2,memory=1GiB,sqlite_pool=3",
        "cpu": 2,
        "memory": "1GiB",
        "memory_bytes": 1024**3,
        "sqlite_pool": 3,
    },
    "process": {
        "cpu_affinity_cpus": [0, 1],
        "address_space_limit_bytes": [1024**3, 1024**3],
        "cgroup": {"memory_max_raw": "max", "memory_max_bytes": None},
    },
    "startup": {
        "sqlite_pool_max_connections": 3,
        "source": "src/lib.rs:SQLITE_POOL_MAX_CONNECTIONS_DEFAULT",
    },
    "enforcement": ["sched_setaffinity", "RLIMIT_AS"],
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
    resource_observation: dict[str, object] | None = None,
    status: str = "measured",
    reason: str | None = None,
) -> dict[str, object]:
    success_request = {
        "samples": request_samples,
        "mean_ms": request_p95 / 2,
        "p95_ms": request_p95,
        "throughput_rps": 0.05,
    }
    scenario_records = [
        {
            "name": scenario["name"],
            "expected": {
                "outcome": scenario["expected_outcome"],
                "status_class": scenario["expected_status_class"],
            },
            "actual": {
                "outcomes": {"success": request_samples, "reject": 0, "error": 0},
                "success_request": success_request,
            },
            "success_request": success_request,
            "request": success_request,
            "statuses": {"business:200": request_samples},
            "outcome_counts": {"business:success": request_samples},
            "response_evidence": [],
            "errors": {},
        }
        for scenario in SCENARIOS
    ]
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
        "success_request": success_request,
        "unrelated_cohort": {
            "samples": unrelated_samples,
            "mean_ms": unrelated_p95,
            "p95_ms": unrelated_p95,
            "throughput_rps": 0.01,
        },
        "statuses": {"business:200": request_samples},
        "outcome_counts": {"business:success": request_samples},
        "business_outcomes": {"success": request_samples, "reject": 0, "error": 0},
        "response_evidence": [],
        "errors": {},
        "phases": {"total": {"count": request_samples, "sum_seconds": 1.0}},
        "phase_schema": {
            "status": "supported",
            "kind": HARNESS.PHASE_SCHEMA_OPERATION,
            "metric_name": "hikari_http_request_duration_seconds",
            "selector": dict(HARNESS.PHASE_METRIC_SELECTOR),
            "phase_keys": ["total"],
            "coverage_count": request_samples,
            "coverage_phase": "total",
        },
        "accepted_progress": {
            "source": "maintenance_or_recovery_response_field_only",
            "responses": 0,
            "reported": 0,
        },
        "cohort": cohort or COHORT,
        "scenarios": scenario_records,
        "scenario_contract": SCENARIOS,
        "credential_mapping": MAPPING,
        "resource_observation": resource_observation or RESOURCES,
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
                "round-1/baseline/business/success.samples_below_minimum",
                "round-1/baseline/scenarios/serial/success.samples_below_minimum",
                "round-1/baseline/scenarios/cross_account_concurrency_4/success.samples_below_minimum",
                "round-1/baseline/scenarios/cross_account_concurrency_8/success.samples_below_minimum",
                "round-1/baseline/scenarios/same_account_queue/success.samples_below_minimum",
            ],
        )

    def test_rejects_all_429_business_samples_even_when_transport_samples_are_adequate(self) -> None:
        result = measured_result(
            variant="candidate",
            round_number=1,
            request_p95=100,
            unrelated_p95=100,
        )
        result["request"] = {
            "samples": 30,
            "mean_ms": 100,
            "p95_ms": 100,
            "throughput_rps": 0.05,
        }
        result["success_request"] = {
            "samples": 0,
            "mean_ms": None,
            "p95_ms": None,
            "throughput_rps": 0.0,
        }
        result["business_outcomes"] = {"success": 0, "reject": 30, "error": 0}
        result["statuses"] = {"business:429": 30}
        result["phases"] = {}
        result["phase_schema"] = {
            "status": "unsupported",
            "kind": HARNESS.PHASE_SCHEMA_UNSUPPORTED,
            "reason": "selector_no_matching_phase_series",
            "coverage_count": 0,
        }
        for scenario in result["scenarios"]:
            scenario["actual"] = {
                "outcomes": {"success": 0, "reject": 30, "error": 0},
                "success_request": {
                    "samples": 0,
                    "mean_ms": None,
                    "p95_ms": None,
                    "throughput_rps": 0.0,
                },
            }
        comparison = HARNESS.compare_round(
            paired_round(
                1,
                measured_result(variant="baseline", round_number=1, request_p95=100, unrelated_p95=100),
                result,
            ),
            min_samples=30,
        )

        self.assertFalse(comparison["comparable"])
        self.assertIn("round-1/candidate/business/success.samples_missing", comparison["reasons"])
        self.assertIn("round-1/candidate/business/429_or_error_only", comparison["reasons"])
        self.assertIn(
            "round-1/candidate/phase_schema/selector_no_matching_phase_series",
            comparison["reasons"],
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
    def test_http_reject_is_not_a_success_sample_and_keeps_fixed_429_evidence(self) -> None:
        recorder = HARNESS.SampleRecorder()
        recorder.add_status(
            "business",
            429,
            12.0,
            b'{"error":"quota_exhausted","message":"quota exhausted for this token"}',
            {"Content-Type": "application/json", "Retry-After": "7", "Authorization": "Bearer secret"},
        )

        result = recorder.result(1)

        self.assertEqual(result["request"]["samples"], 1)
        self.assertEqual(result["success_request"]["samples"], 0)
        self.assertEqual(result["business_outcomes"], {"success": 0, "reject": 1, "error": 0})
        self.assertEqual(result["response_evidence"][0]["body"]["error"], "quota_exhausted")
        self.assertEqual(result["response_evidence"][0]["headers"]["retry-after"], "7")
        self.assertNotIn("authorization", result["response_evidence"][0]["headers"])

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
        self.assertEqual(HARNESS.DEFAULT_RESOURCES, "cpu=2,memory=1GiB,sqlite_pool=3")
        spec = HARNESS.parse_resource_spec(HARNESS.DEFAULT_RESOURCES)

        self.assertEqual(spec["cpu"], 2)
        self.assertEqual(spec["memory_bytes"], 1024**3)
        self.assertEqual(spec["sqlite_pool"], 3)

    def test_resource_observation_identifies_process_rlimit_scope(self) -> None:
        self.assertEqual(RESOURCES["scope"], "launcher_process_and_inherited_descendants")
        process = RESOURCES["process"]
        self.assertEqual(process["address_space_limit_bytes"], [1024**3, 1024**3])
        self.assertEqual(process["cgroup"]["memory_max_raw"], "max")
        self.assertIsNone(process["cgroup"]["memory_max_bytes"])

    def test_phase_metric_selector_accepts_base_and_candidate_label_shapes(self) -> None:
        base_labels = {
            "route_family": "http_search",
            "method": "POST",
            "outcome": "success",
            "phase": "total",
        }
        candidate_labels = {**base_labels, "operation": "search"}
        wrong_operation = {**base_labels, "operation": "mcp"}

        self.assertTrue(HARNESS.phase_metric_matches(base_labels))
        self.assertTrue(HARNESS.phase_metric_matches(candidate_labels))
        self.assertFalse(HARNESS.phase_metric_matches(wrong_operation))
        self.assertTrue(
            HARNESS.phase_metric_matches_schema(base_labels, HARNESS.PHASE_SCHEMA_LEGACY)
        )
        self.assertTrue(
            HARNESS.phase_metric_matches_schema(candidate_labels, HARNESS.PHASE_SCHEMA_OPERATION)
        )
        self.assertFalse(
            HARNESS.phase_metric_matches_schema(base_labels, HARNESS.PHASE_SCHEMA_OPERATION)
        )

    def test_sqlite_pool_mismatch_remains_fail_closed(self) -> None:
        mismatch = {
            **RESOURCES,
            "status": "mismatch",
            "requested": {
                **RESOURCES["requested"],
                "raw": "cpu=2,memory=1GiB,sqlite_pool=4",
                "sqlite_pool": 4,
            },
            "errors": ["sqlite_pool_requested_4_observed_3"],
        }
        comparison = HARNESS.compare_round(
            paired_round(
                1,
                measured_result(
                    variant="baseline",
                    round_number=1,
                    request_p95=100,
                    unrelated_p95=100,
                    resource_observation=mismatch,
                ),
                measured_result(
                    variant="candidate",
                    round_number=1,
                    request_p95=100,
                    unrelated_p95=100,
                ),
            ),
            min_samples=1,
            min_control_samples=1,
        )

        self.assertEqual(comparison["status"], "unvalidated")
        self.assertIn("round-1/baseline/resources/constraints_unverified", comparison["reasons"])

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
