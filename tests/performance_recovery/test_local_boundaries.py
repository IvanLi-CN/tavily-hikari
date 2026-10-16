#!/usr/bin/env python3
"""Regression tests for the mock-only local performance boundary harness."""

from __future__ import annotations

import importlib.util
import hashlib
import json
import urllib.request
from pathlib import Path
import sqlite3
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
HARNESS_PATH = ROOT / "scripts" / "perf-local-boundaries.py"
SPEC = importlib.util.spec_from_file_location("perf_local_boundaries", HARNESS_PATH)
assert SPEC is not None and SPEC.loader is not None
HARNESS = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = HARNESS
SPEC.loader.exec_module(HARNESS)


COHORT = HARNESS.logical_cohort(
    tuple(f"subject-{index}" for index in range(1, 15)),
    tuple(f"token-{index}" for index in range(1, 15)),
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
        for index in range(1, 15)
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
BINARY_PROVENANCE = {
    "status": "verified",
    "reason": None,
    "manifest_path": "/tmp/perf-build.json",
    "manifest_sha256": "a" * 64,
    "source_sha": "candidate-ref",
    "build_input_sha": "candidate-ref",
    "binary_path": "/tmp/perf-service",
    "binary_sha256": "b" * 64,
    "compiler": "rustc 1.91.0",
    "profile": "release",
    "flags": ["--locked"],
    "ci": {
        "run_id": "123",
        "run_head_sha": "candidate-ref",
        "checkout_sha": "candidate-ref",
    },
}
SERVICE_COMMAND_BINDING = {
    "status": "explicit_placeholder",
    "source": "{binary}",
}
SYSTEM_DIAGNOSTICS = {
    "status": "validated",
    "reason": None,
    "cadence_seconds": 1.0,
    "sample_count": 2,
    "dropped_samples": 0,
    "missing": [],
    "errors": [],
    "samples": [],
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
                "status_counts": {"200": request_samples, "429": 0, "other": 0},
                "success_request": success_request,
            },
            "success_request": success_request,
            "request": success_request,
            "credential_ids": [
                f"{index + 1:04d}" for index in scenario["credential_indices"]
            ],
            "credential_labels": [
                f"token-{index + 1}" for index in scenario["credential_indices"]
            ],
            "billing_subjects": [
                f"token:{index + 1:04d}" for index in scenario["credential_indices"]
            ],
            "subjects": [
                f"subject-{index + 1}" for index in scenario["subject_indices"]
            ],
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
        "business_outcomes": {"success": request_samples, "reject": 0, "error": 0},
        "business_statuses": {"200": request_samples, "429": 0, "other": 0},
        "unrelated_cohort": {
            "samples": unrelated_samples,
            "mean_ms": unrelated_p95,
            "p95_ms": unrelated_p95,
            "throughput_rps": 0.01,
        },
        "statuses": {"business:200": request_samples},
        "outcome_counts": {"business:success": request_samples},
        "business_outcomes": {"success": request_samples, "reject": 0, "error": 0},
        "business_statuses": {"200": request_samples, "429": 0, "other": 0},
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
        "phase_observation": {
            "measurement_start": {
                "schema": {"status": "supported"},
                "values": {},
                "observability": {"dropped": {}, "flush": {}},
            },
            "after_join_flush": {
                "schema": {"status": "supported"},
                "values": {},
                "observability": {"dropped": {}, "flush": {}},
            },
            "observability_delta": {"dropped": {}, "flush": {}},
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
        "binary_provenance": BINARY_PROVENANCE,
        "service_command_binding": SERVICE_COMMAND_BINDING,
        "system_diagnostics": SYSTEM_DIAGNOSTICS,
        "warmup": {
            "duration_seconds": 1,
            "request": success_request,
            "business_statuses": {"200": request_samples, "429": 0, "other": 0},
            "business_outcomes": {"success": request_samples, "reject": 0, "error": 0},
            "response_evidence": [],
        },
        "fixture_settings": dict(HARNESS.SYNTHETIC_FIXTURE_SETTINGS),
        "mock_upstream": {
            "before": {
                "status": "ready",
                "url_kind": "loopback_synthetic_mock",
                "request_count": 0,
                "response_statuses": {},
                "response_usage_credits": 1,
                "response_contract": "results + usage.credits=1",
            },
            "after": {
                "status": "ready",
                "url_kind": "loopback_synthetic_mock",
                "request_count": request_samples,
                "response_statuses": {"200": request_samples},
                "response_usage_credits": 1,
                "response_contract": "results + usage.credits=1",
            },
            "response_usage_credits": 1,
        },
    }


def paired_round(
    round_number: int,
    baseline: dict[str, object],
    candidate: dict[str, object],
) -> dict[str, object]:
    return {"round": round_number, "baseline": baseline, "candidate": candidate}


class LocalBoundaryComparatorTests(unittest.TestCase):
    def test_identity_reuse_is_unvalidated_even_when_samples_are_healthy(self) -> None:
        result = measured_result(
            variant="candidate",
            round_number=1,
            request_p95=100,
            unrelated_p95=100,
        )
        cross_account = next(
            record for record in result["scenarios"] if record["name"] == "cross_account_concurrency_4"
        )
        cross_account["credential_ids"] = ["0001"] * 4
        cross_account["billing_subjects"] = ["token:0001"] * 4
        cross_account["subjects"] = ["subject-1"] * 4

        reasons = HARNESS.measurement_reasons(1, "candidate", result, min_samples=30)

        self.assertIn(
            "round-1/candidate/scenarios/cross_account_concurrency_4/identity/"
            "billing_subjects_reuse_mismatch",
            reasons,
        )
        self.assertTrue(
            any(
                reason.endswith(
                    "identity/credential_reused_with_cross_account_concurrency_4"
                )
                for reason in reasons
            )
        )
        self.assertTrue(
            any(
                reason.endswith("identity/subject_reused_with_cross_account_concurrency_4")
                for reason in reasons
            )
        )

    def test_missing_runtime_provenance_is_unvalidated(self) -> None:
        result = measured_result(
            variant="candidate",
            round_number=1,
            request_p95=100,
            unrelated_p95=100,
        )
        result.pop("binary_provenance")
        result["system_diagnostics"] = {
            "status": "unavailable",
            "reason": "system_diagnostics_incomplete",
        }

        reasons = HARNESS.measurement_reasons(1, "candidate", result, min_samples=30)

        self.assertIn("round-1/candidate/binary_provenance/missing", reasons)
        self.assertIn(
            "round-1/candidate/system_diagnostics/system_diagnostics_incomplete", reasons
        )

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
    def test_mock_upstream_evidence_counts_requests_and_usage(self) -> None:
        with HARNESS.MockUpstream(0) as mock:
            with urllib.request.urlopen(f"{mock.url}/", timeout=2) as response:
                self.assertEqual(response.status, 200)
                self.assertIn(b"usage", response.read())

            evidence = mock.evidence()

        self.assertEqual(evidence["request_count"], 1)
        self.assertEqual(evidence["response_statuses"], {"200": 1})
        self.assertEqual(evidence["response_usage_credits"], 1)
        self.assertEqual(evidence["response_contract"], "results + usage.credits=1")

    def test_transport_error_is_counted_in_business_outcomes(self) -> None:
        recorder = HARNESS.SampleRecorder()
        recorder.add_error("business", TimeoutError("bounded test timeout"))

        result = recorder.result(1)

        self.assertEqual(result["business_outcomes"], {"success": 0, "reject": 0, "error": 1})
        self.assertEqual(result["outcome_counts"], {"business:error": 1})
        self.assertEqual(result["errors"], {"business:TimeoutError": 1})

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
        self.assertEqual(result["business_statuses"], {"200": 0, "429": 1, "other": 0})
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
            for index in range(1, 15)
        )
        subjects = tuple(f"subject-{index}" for index in range(1, 15))
        bindings = {
            scenario["name"]: HARNESS.scenario_bindings(scenario["name"], credentials, subjects)
            for scenario in scenarios
        }

        self.assertEqual(
            [scenario["credential_indices"] for scenario in scenarios],
            [[0], [1, 2, 3, 4], [5, 6, 7, 8, 9, 10, 11, 12], [13] * 8],
        )
        self.assertEqual(
            [scenario["subject_indices"] for scenario in scenarios],
            [[0], [1, 2, 3, 4], [5, 6, 7, 8, 9, 10, 11, 12], [13] * 8],
        )
        for name in ("serial", "cross_account_concurrency_4", "cross_account_concurrency_8"):
            scenario_credentials, scenario_subjects = bindings[name]
            self.assertEqual(
                len({credential.billing_subject for credential in scenario_credentials}),
                len(scenario_credentials),
            )
            self.assertEqual(len(set(scenario_subjects)), len(scenario_subjects))

        same_credentials, same_subjects = bindings["same_account_queue"]
        self.assertEqual(len({credential.billing_subject for credential in same_credentials}), 1)
        self.assertEqual(len(set(same_subjects)), 1)

        scenario_owners = {
            name: {credential.billing_subject for credential in bindings[name][0]}
            for name in (
                "serial",
                "cross_account_concurrency_4",
                "cross_account_concurrency_8",
                "same_account_queue",
            )
        }
        subject_owners = {
            name: set(bindings[name][1])
            for name in (
                "serial",
                "cross_account_concurrency_4",
                "cross_account_concurrency_8",
                "same_account_queue",
            )
        }
        for owners in (scenario_owners, subject_owners):
            for left_name, left_subjects in owners.items():
                for right_name, right_subjects in owners.items():
                    if left_name < right_name:
                        self.assertTrue(left_subjects.isdisjoint(right_subjects))

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

    def test_observability_counter_snapshots_are_bounded_and_delta_is_explicit(self) -> None:
        body = "\n".join(
            [
                'hikari_observability_dropped_total{reason="queue"} 11',
                'hikari_observability_dropped_total{reason="cardinality"} 2',
                'hikari_observability_flush_total{outcome="success",reason="scrape"} 7',
                'hikari_observability_flush_total{outcome="error",reason="scrape"} 1',
            ]
        )

        dropped = HARNESS.parse_counter_family(
            body, "hikari_observability_dropped_total", ("reason",)
        )
        flush = HARNESS.parse_counter_family(
            body, "hikari_observability_flush_total", ("outcome", "reason")
        )

        self.assertEqual(dropped, {"cardinality": 2.0, "queue": 11.0})
        self.assertEqual(flush, {"error|scrape": 1.0, "success|scrape": 7.0})
        self.assertEqual(
            HARNESS.observability_delta(
                {"observability": {"dropped": {"queue": 3.0}, "flush": {"success|scrape": 4.0}}},
                {"observability": {"dropped": {"queue": 14.0}, "flush": {"success|scrape": 7.0}}},
            ),
            {"dropped": {"queue": 11.0}, "flush": {"success|scrape": 3.0}},
        )

    def test_metrics_collector_records_continued_success_after_history_truncation(self) -> None:
        supported = {
            "values": {"total": {"count": 1.0, "sum_seconds": 0.1}},
            "schema": {"status": "supported", "kind": HARNESS.PHASE_SCHEMA_OPERATION},
            "counters": {"http_requests": {}},
            "observability": {"dropped": {}, "flush": {}},
        }
        with mock.patch.object(HARNESS, "collect_phase_snapshot", return_value=supported):
            collector = HARNESS.MetricsCollector(0, "test-token", 1.0)
            for _ in range(HARNESS.MAX_METRICS_SCRAPES + 345):
                collector.scrape("cadence")

        evidence = collector.evidence()

        self.assertEqual(evidence["scrape_attempts"], 601)
        self.assertEqual(evidence["scrape_successes"], 601)
        self.assertEqual(evidence["scrape_failures"], 0)
        self.assertEqual(evidence["scrape_failure_reasons"], {})
        self.assertEqual(evidence["errors"], [])
        self.assertEqual(evidence["history_retained"], HARNESS.MAX_METRICS_SCRAPES)
        self.assertTrue(evidence["history_truncated"])
        self.assertEqual(evidence["history_truncated_count"], 345)
        self.assertEqual(evidence["history_sequence_range"], [346, 601])

    def test_metrics_collector_separates_scrape_failure_from_history_truncation(self) -> None:
        snapshots = iter(
            [
                {
                    "values": {},
                    "schema": {"status": "unsupported", "reason": "metrics_unavailable"},
                    "counters": {"http_requests": {}},
                    "observability": {"dropped": {}, "flush": {}},
                },
                {
                    "values": {"total": {"count": 1.0, "sum_seconds": 0.1}},
                    "schema": {"status": "supported", "kind": HARNESS.PHASE_SCHEMA_OPERATION},
                    "counters": {"http_requests": {}},
                    "observability": {"dropped": {}, "flush": {}},
                },
            ]
        )
        with mock.patch.object(HARNESS, "collect_phase_snapshot", side_effect=snapshots):
            collector = HARNESS.MetricsCollector(0, "test-token", 1.0)
            collector.scrape("measurement_start")
            collector.scrape("after_join_flush")

        evidence = collector.evidence()

        self.assertEqual(evidence["scrape_attempts"], 2)
        self.assertEqual(evidence["scrape_successes"], 1)
        self.assertEqual(evidence["scrape_failures"], 1)
        self.assertEqual(evidence["scrape_failure_reasons"], {"metrics_unavailable": 1})
        self.assertEqual(
            evidence["errors"],
            [{"boundary": "measurement_start", "reason": "metrics_unavailable"}],
        )
        self.assertFalse(evidence["history_truncated"])
        self.assertEqual(evidence["history_truncated_count"], 0)

    def test_service_readiness_records_listener_and_health_separately(self) -> None:
        class Response:
            status = 503

            @staticmethod
            def read() -> bytes:
                return b"starting"

        class Connection:
            def request(self, *_args: object, **_kwargs: object) -> None:
                return None

            @staticmethod
            def getresponse() -> Response:
                return Response()

            @staticmethod
            def close() -> None:
                return None

        with mock.patch.object(HARNESS.http.client, "HTTPConnection", return_value=Connection()):
            readiness = HARNESS.wait_for_service(12345, 1)

        self.assertTrue(readiness["listener_ready"])
        self.assertFalse(readiness["health_ready"])
        self.assertEqual(readiness["http_status"], 503)

    def test_token_bootstrap_retries_only_explicit_deferred_status(self) -> None:
        class Response:
            def __init__(self, status: int, body: bytes, retry_after: str | None = None) -> None:
                self.status = status
                self._body = body
                self.headers = {"Retry-After": retry_after} if retry_after is not None else {}

            def read(self) -> bytes:
                return self._body

        responses = iter(
            [
                Response(503, b"deferred", "0"),
                Response(201, b'{"token":"th-abcd-secret"}'),
            ]
        )

        class Connection:
            def request(self, *_args: object, **_kwargs: object) -> None:
                return None

            @staticmethod
            def getresponse() -> Response:
                return next(responses)

            @staticmethod
            def close() -> None:
                return None

        with mock.patch.object(HARNESS.http.client, "HTTPConnection", return_value=Connection()):
            credentials, evidence = HARNESS.bootstrap_business_tokens(12345, ("token-1",))

        self.assertEqual([credential.token_id for credential in credentials], ["abcd"])
        self.assertEqual(evidence["status"], "ready")
        self.assertEqual([item["status"] for item in evidence["attempts"]], [503, 201])
        self.assertEqual(evidence["attempts"][0]["retry_basis"], "http_503_retry_after")
        self.assertEqual(evidence["attempts"][0]["decision"], "retry")
        self.assertEqual(evidence["attempts"][1]["decision"], "accept")

    def test_token_bootstrap_does_not_retry_503_without_retry_after(self) -> None:
        class Response:
            status = 503
            headers: dict[str, str] = {}

            @staticmethod
            def read() -> bytes:
                return b"deferred"

        class Connection:
            def request(self, *_args: object, **_kwargs: object) -> None:
                return None

            @staticmethod
            def getresponse() -> Response:
                return Response()

            @staticmethod
            def close() -> None:
                return None

        with mock.patch.object(HARNESS.http.client, "HTTPConnection", return_value=Connection()):
            credentials, evidence = HARNESS.bootstrap_business_tokens(12345, ("token-1",))

        self.assertEqual(credentials, ())
        self.assertEqual(evidence["status"], "failed")
        self.assertEqual(evidence["reason"], "token_bootstrap_status_503")
        self.assertEqual(len(evidence["attempts"]), 1)
        self.assertFalse(evidence["attempts"][0]["retryable"])

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
            evidence = writer.evidence()

            self.assertLessEqual(path.stat().st_size, 64)
            self.assertNotIn("tvly-secret-value", content)
            self.assertNotIn("raw-token", content)
            self.assertIn("<LOG_TRUNCATED_HEAD_TAIL>", content)
            self.assertTrue(content.endswith("x" * 10))
            self.assertTrue(evidence["truncated"])
            self.assertGreater(evidence["dropped_bytes"], 0)
            self.assertEqual(evidence["strategy"], "head_tail")
            self.assertEqual(len(evidence["retained_ranges"]), 2)
            self.assertEqual(len(evidence["dropped_ranges"]), 1)
            self.assertTrue(evidence["closed"])

            split_path = Path(temporary) / "split.log"
            split_writer = HARNESS.BoundedLogWriter(split_path, max_bytes=128)
            split_writer.write(b"Authorization: Bearer tvly-")
            split_writer.write(b"split-secret-value\n")
            split_writer.close()
            split_content = split_path.read_text(encoding="utf-8")
            self.assertNotIn("split-secret-value", split_content)
            self.assertIn("Bearer <REDACTED>", split_content)

    def test_binary_manifest_requires_hash_and_build_provenance(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "service"
            binary.write_bytes(b"synthetic-service")
            manifest = root / "manifest.json"
            manifest.write_text(
                json.dumps(
                    {
                        "source_sha": "candidate-ref",
                        "run_head_sha": "pull-request-head",
                        "checkout_sha": "candidate-ref",
                        "ci": {
                            "run_id": "38766352037",
                            "run_head_sha": "pull-request-head",
                            "checkout_sha": "candidate-ref",
                        },
                        "binary": {
                            "path": binary.name,
                            "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                        },
                        "build": {
                            "compiler": "rustc 1.91.0",
                            "profile": "release",
                            "flags": ["--locked", "-j2"],
                        },
                    }
                ),
                encoding="utf-8",
            )

            evidence = HARNESS.binary_manifest_evidence(
                str(manifest), root / "repo", "candidate", "candidate-ref"
            )
            binding = HARNESS.service_command_binary_binding("{binary} --help", evidence)

            self.assertEqual(evidence["status"], "verified")
            self.assertEqual(evidence["source_sha"], "candidate-ref")
            self.assertEqual(evidence["build_input_sha"], "candidate-ref")
            self.assertEqual(evidence["ci"]["run_head_sha"], "pull-request-head")
            self.assertEqual(evidence["ci"]["checkout_sha"], "candidate-ref")
            self.assertEqual(binding["status"], "explicit_placeholder")

            manifest_text = manifest.read_text(encoding="utf-8")
            manifest.write_text(
                manifest_text.replace(
                    '"source_sha": "candidate-ref"', '"source_sha": "other-ref"'
                ).replace(
                    '"checkout_sha": "candidate-ref"', '"checkout_sha": "other-ref"'
                ),
                encoding="utf-8",
            )
            mismatch = HARNESS.binary_manifest_evidence(
                str(manifest), root / "repo", "candidate", "candidate-ref"
            )
            self.assertEqual(mismatch["status"], "invalid")
            self.assertEqual(mismatch["reason"], "binary_manifest_source_mismatch")

    def test_system_snapshot_reports_fixed_sources_and_missing_files(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            proc_root = Path(temporary)
            (proc_root / "pressure").mkdir()
            (proc_root / "42").mkdir()
            (proc_root / "stat").write_text(
                "cpu 10 2 3 40 5 6 7 8 0 0\n", encoding="ascii"
            )
            (proc_root / "pressure" / "cpu").write_text(
                "some avg10=0.10 avg60=0.20 avg300=0.30 total=4\n", encoding="ascii"
            )
            (proc_root / "pressure" / "io").write_text(
                "full avg10=0.01 avg60=0.02 avg300=0.03 total=2\n", encoding="ascii"
            )
            (proc_root / "loadavg").write_text("0.10 0.20 0.30 2/100 42\n", encoding="ascii")
            (proc_root / "42" / "stat").write_text(
                "42 (service worker) S " + " ".join(["0"] * 21) + "\n", encoding="ascii"
            )
            (proc_root / "42" / "io").write_text(
                "read_bytes: 12\nwrite_bytes: 34\n", encoding="ascii"
            )
            (proc_root / "diskstats").write_text(
                "8 0 vda 1 0 2 3 4 0 5 6 0 7 8 9\n", encoding="ascii"
            )

            snapshot = HARNESS.collect_system_snapshot(42, proc_root)

            self.assertEqual(snapshot["cpu"]["steal"], 8)
            self.assertEqual(snapshot["load"]["runnable"], 2)
            self.assertEqual(snapshot["process_io"]["read_bytes"], 12)
            self.assertIn("storage", snapshot)
            self.assertEqual(snapshot["errors"], [])

            collector = HARNESS.SystemDiagnosticsCollector(
                42, interval_seconds=1.0, proc_root=proc_root
            )
            collector._capture()
            (proc_root / "stat").write_text(
                "cpu 20 2 3 50 5 6 7 10 0 0\n", encoding="ascii"
            )
            collector._capture()
            diagnostics = collector.evidence()
            self.assertEqual(diagnostics["status"], "validated")
            self.assertEqual(diagnostics["sample_count"], 2)
            self.assertIsNotNone(diagnostics["samples"][1]["cpu_delta"]["steal_percent"])

            (proc_root / "pressure" / "io").unlink()
            incomplete = HARNESS.collect_system_snapshot(42, proc_root)
            self.assertIn("psi_io:FileNotFoundError", incomplete["errors"])

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
            for field in (
                "request",
                "success_request",
                "unrelated_cohort",
                "statuses",
                "business_statuses",
                "outcome_counts",
                "business_outcomes",
                "response_evidence",
                "errors",
                "phases",
                "phase_schema",
                "phase_observation",
                "warmup",
                "fixture_settings",
                "mock_upstream",
                "binary_provenance",
                "service_command_binding",
                "system_diagnostics",
                "logs",
            ):
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
