#!/usr/bin/env python3
"""Regression tests for the mock-only local performance boundary harness."""

from __future__ import annotations

import io
import importlib.util
import errno
import hashlib
import json
import os
import shlex
import subprocess
import threading
import time
import urllib.request
from contextlib import closing
from pathlib import Path
import sqlite3
import sys
import tempfile
import unittest
import warnings
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
    "pid": 42,
    "cadence_seconds": 1.0,
    "sample_count": 2,
    "dropped_samples": 0,
    "missing": [],
    "errors": [],
    "samples": [],
}
SERVICE_PROCESS = {
    "status": "verified",
    "reason": None,
    "launcher_pid": 41,
    "service_pid": 42,
    "service_exe": "/tmp/perf-service",
    "service_sha256": "b" * 64,
    "descendant_pids": [42],
    "candidate_processes": [],
    "errors": [],
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
        "service_process": SERVICE_PROCESS,
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


def create_test_git_repo(parent: Path) -> tuple[Path, str]:
    source = parent / "source"
    source.mkdir()
    subprocess.run(["git", "init", "--quiet", str(source)], check=True)
    subprocess.run(
        ["git", "-C", str(source), "config", "user.email", "test@example.invalid"],
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(source), "config", "user.name", "Boundary Test"],
        check=True,
    )
    (source / "fixture.txt").write_text("synthetic fixture\n", encoding="ascii")
    subprocess.run(["git", "-C", str(source), "add", "fixture.txt"], check=True)
    subprocess.run(
        ["git", "-C", str(source), "commit", "--quiet", "-m", "test fixture"],
        check=True,
    )
    revision = subprocess.check_output(
        ["git", "-C", str(source), "rev-parse", "HEAD"], text=True
    ).strip()
    return source, revision


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
        result.pop("service_process")
        result["system_diagnostics"] = {
            "status": "unavailable",
            "reason": "system_diagnostics_incomplete",
        }

        reasons = HARNESS.measurement_reasons(1, "candidate", result, min_samples=30)

        self.assertIn("round-1/candidate/binary_provenance/missing", reasons)
        self.assertIn("round-1/candidate/service_process/missing", reasons)
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


class MetricsCadenceContractTests(unittest.TestCase):
    def test_metrics_enabled_defaults_to_one_second_bounded_scrapes(self) -> None:
        self.assertEqual(HARNESS.DEFAULT_METRICS_SCRAPE_INTERVAL_SECONDS, 1.0)
        HARNESS.validate_metrics_scrape_interval(1.0, True)

    def test_metrics_enabled_rejects_boundary_only_and_subsecond_scrapes(self) -> None:
        with self.assertRaisesRegex(
            SystemExit, "metrics-enabled runs require a positive bounded metrics scrape cadence"
        ):
            HARNESS.validate_metrics_scrape_interval(0.0, True)
        with self.assertRaisesRegex(
            SystemExit, "metrics scrape interval must be 0 when disabled or between 1 and 60 seconds"
        ):
            HARNESS.validate_metrics_scrape_interval(0.5, True)

    def test_disabled_metrics_can_keep_boundary_only_mode(self) -> None:
        HARNESS.validate_metrics_scrape_interval(0.0, False)

    def test_metrics_collector_does_not_catch_up_after_a_slow_scrape(self) -> None:
        class StopEvent:
            def __init__(self) -> None:
                self.waits: list[float] = []

            def wait(self, timeout: float) -> bool:
                self.waits.append(timeout)
                return len(self.waits) >= 3

        stop_event = StopEvent()
        collector = HARNESS.MetricsCollector(0, "test-token", 1.0)
        collector._stop_event = stop_event
        with mock.patch.object(collector, "scrape"):
            collector._run()

        self.assertEqual(stop_event.waits, [1.0, 1.0, 1.0])


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

        self.assertEqual(evidence["cadence_mode"], "bounded_post_scrape")
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

        self.assertEqual(evidence["cadence_mode"], "bounded_post_scrape")
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
            with closing(sqlite3.connect(database)) as connection:
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
            with closing(sqlite3.connect(database)) as connection:
                with connection:
                    connection.execute(
                        "INSERT INTO user_token_bindings (token_id, user_id) VALUES ('0002', 'user-2')"
                    )
            self.assertEqual(HARNESS.verify_credential_bindings(database, credentials)["status"], "failed")

    def test_bound_fixture_uses_management_binding_and_records_account_quota(self) -> None:
        labels = tuple(f"token-{index}" for index in range(1, 15))
        responses = iter(
            [
                type(
                    "Response",
                    (),
                    {
                        "status": 500,
                        "headers": {},
                        "read": lambda self: b"database is locked",
                    },
                )(),
                *[
                    type(
                        "Response",
                        (),
                        {
                            "status": 201,
                            "headers": {},
                            "read": lambda self, index=index: json.dumps(
                                {"token": f"th-{index:04d}-{'x' * 24}"}
                            ).encode(),
                        },
                    )()
                    for index in range(1, 15)
                ]
            ]
        )

        class Connection:
            requests: list[tuple[str, str]] = []
            database: Path | None = None
            token_index = 0

            def request(self, method: str, path: str, **_kwargs: object) -> None:
                self.requests.append((method, path))

            def getresponse(self) -> object:
                response = next(responses)
                if response.status != 201:
                    return response
                Connection.token_index += 1
                index = Connection.token_index
                token_id = f"{index:04d}"
                account_id = f"perf-account-{index:02d}"
                assert self.database is not None
                with closing(sqlite3.connect(self.database)) as database:
                    with database:
                        database.execute(
                            """
                            INSERT INTO auth_tokens
                                (id, secret, enabled, note, total_requests, created_at)
                            VALUES (?, ?, 1, ?, 0, 1)
                            """,
                            (token_id, "x" * 24, f"perf-local-bound-{index:02d}"),
                        )
                        database.execute(
                            """
                            INSERT INTO user_token_bindings
                                (user_id, token_id, created_at, updated_at)
                            VALUES (?, ?, 1, 1)
                            """,
                            (account_id, token_id),
                        )
                return response

            @staticmethod
            def close() -> None:
                return None

        with tempfile.TemporaryDirectory() as temporary:
            database = Path(temporary) / "bound-fixture.db"
            with closing(sqlite3.connect(database)) as connection:
                connection.executescript(
                    """
                    PRAGMA foreign_keys = ON;
                    CREATE TABLE users (
                        id TEXT PRIMARY KEY,
                        display_name TEXT,
                        username TEXT,
                        avatar_template TEXT,
                        active INTEGER NOT NULL DEFAULT 1,
                        debug_info_shared INTEGER NOT NULL DEFAULT 0,
                        created_at INTEGER NOT NULL,
                        updated_at INTEGER NOT NULL,
                        last_login_at INTEGER
                    );
                    CREATE TABLE auth_tokens (
                        id TEXT PRIMARY KEY,
                        secret TEXT NOT NULL,
                        enabled INTEGER NOT NULL DEFAULT 1,
                        note TEXT,
                        group_name TEXT,
                        total_requests INTEGER NOT NULL DEFAULT 0,
                        created_at INTEGER NOT NULL,
                        last_used_at INTEGER,
                        deleted_at INTEGER
                    );
                    CREATE TABLE user_token_bindings (
                        user_id TEXT NOT NULL,
                        token_id TEXT NOT NULL UNIQUE,
                        created_at INTEGER NOT NULL,
                        updated_at INTEGER NOT NULL,
                        PRIMARY KEY (user_id, token_id),
                        FOREIGN KEY (user_id) REFERENCES users(id),
                        FOREIGN KEY (token_id) REFERENCES auth_tokens(id)
                    );
                    CREATE TABLE account_quota_limits (
                        user_id TEXT PRIMARY KEY,
                        business_calls_1h_limit INTEGER NOT NULL,
                        daily_credits_limit INTEGER NOT NULL,
                        monthly_credits_limit INTEGER NOT NULL,
                        monthly_broken_limit INTEGER NOT NULL DEFAULT 5,
                        monthly_blocked_key_limit_delta INTEGER NOT NULL DEFAULT 0,
                        inherits_defaults INTEGER NOT NULL DEFAULT 1,
                        created_at INTEGER NOT NULL,
                        updated_at INTEGER NOT NULL,
                        FOREIGN KEY (user_id) REFERENCES users(id)
                    );
                    """
                )
            connection = Connection()
            connection.database = database
            with mock.patch.object(
                HARNESS.http.client, "HTTPConnection", return_value=connection
            ):
                with mock.patch.object(HARNESS.time, "sleep"):
                    credentials, evidence = HARNESS.bootstrap_bound_business_accounts(
                        12345, database, labels, 0.25
                    )

            self.assertEqual(evidence["status"], "ready")
            self.assertEqual(len(credentials), 14)
            self.assertEqual(
                [credential.billing_subject for credential in credentials[:2]],
                ["account:perf-account-01", "account:perf-account-02"],
            )
            self.assertEqual(credentials[-1].label, "token-14")
            self.assertEqual(evidence["runtime_binding_check"]["status"], "passed")
            self.assertEqual(evidence["quota_headroom"], HARNESS.BOUND_FIXTURE_QUOTA_HEADROOM)
            self.assertEqual(len(connection.requests), 15)
            self.assertEqual(connection.requests[0], ("POST", "/api/users/perf-account-01/tokens"))
            self.assertEqual(connection.requests[-1], ("POST", "/api/users/perf-account-14/tokens"))
            self.assertEqual(evidence["management_attempts"][0]["retry_basis"], "sqlite_database_locked")
            self.assertEqual(evidence["management_attempts"][0]["decision"], "retry")
            self.assertEqual(
                HARNESS.credential_mapping_evidence(credentials, 0.25)["billing_subject_kind"],
                "account",
            )
            self.assertNotIn(credentials[0].secret, json.dumps(evidence))
            self.assertEqual(
                HARNESS.verify_bound_credential_bindings(database, credentials)["status"],
                "passed",
            )

    def test_bound_fixture_precheck_requires_account_billing_and_one_credit(self) -> None:
        credential = HARNESS.Credential(
            "token-14",
            "th-0014-xxxxxxxxxxxxxxxxxxxxxxxx",
            "0014",
            "account:perf-account-14",
        )

        class Response:
            status = 200

            @staticmethod
            def read() -> bytes:
                return b'{"results":[{"title":"synthetic"}],"usage":{"credits":1}}'

        class Connection:
            def request(self, *_args: object, **_kwargs: object) -> None:
                return None

            @staticmethod
            def getresponse() -> Response:
                return Response()

            @staticmethod
            def close() -> None:
                return None

        with tempfile.TemporaryDirectory() as temporary:
            database = Path(temporary) / "billing.db"
            with closing(sqlite3.connect(database)) as connection:
                with connection:
                    connection.execute(
                        """
                        CREATE TABLE auth_token_logs (
                            id INTEGER PRIMARY KEY AUTOINCREMENT,
                            token_id TEXT NOT NULL,
                            billing_subject TEXT,
                            business_credits INTEGER,
                            result_status TEXT,
                            billing_state TEXT
                        )
                        """
                    )
                    connection.execute(
                        """
                        INSERT INTO auth_token_logs
                            (token_id, billing_subject, business_credits, result_status, billing_state)
                        VALUES (?, ?, ?, ?, ?)
                        """,
                        ("0014", "account:perf-account-14", 1, "success", "pending"),
                    )
            with mock.patch.object(
                HARNESS.http.client, "HTTPConnection", return_value=Connection()
            ):
                evidence = HARNESS.run_bound_fixture_precheck(
                    12345, database, credential, "subject-14"
                )

        self.assertEqual(evidence["status"], "passed")
        self.assertEqual(evidence["response_usage_credits"], 1)
        self.assertEqual(evidence["billing_subject"], "account:perf-account-14")

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

            boundary_path = Path(temporary) / "boundary.log"
            boundary_writer = HARNESS.BoundedLogWriter(boundary_path)
            complete_records = b"prefix\nAuthorization: Bearer <synthetic-secret>\n"
            partial_record = b"+padding" + b"p" * (
                HARNESS.MAX_LOG_REDACTION_CARRY - len(b"+padding")
            )
            boundary_writer.write(
                complete_records + partial_record
            )
            boundary_writer.write(b"B" * len(b"prefix\nAuthorization: Bearer "))
            boundary_writer.close()
            boundary_content = boundary_path.read_text(encoding="utf-8")
            self.assertNotIn("synthetic-secret", boundary_content)
            self.assertIn("Bearer <REDACTED>", boundary_content)
            self.assertGreater(boundary_writer.evidence()["redaction_gap_bytes"], 0)
            self.assertEqual(boundary_writer.evidence()["redaction_gap_count"], 1)

            oversized_path = Path(temporary) / "oversized.log"
            oversized_writer = HARNESS.BoundedLogWriter(oversized_path)
            oversized_writer.write(
                b"Authorization: Bearer " + b"s" * HARNESS.MAX_LOG_REDACTION_CARRY
            )
            oversized_writer.write(b"\n")
            oversized_writer.close()
            oversized_content = oversized_path.read_text(encoding="utf-8")
            self.assertNotIn("s" * 64, oversized_content)
            self.assertGreater(oversized_writer.evidence()["redaction_gap_bytes"], 0)
            self.assertEqual(oversized_writer.evidence()["redaction_gap_count"], 1)

    @unittest.skipUnless(os.name == "posix", "process-group cleanup requires POSIX")
    def test_process_group_cleanup_handles_exited_launcher_with_live_descendant(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            stdout_log = HARNESS.BoundedLogWriter(root / "stdout.log")
            stderr_log = HARNESS.BoundedLogWriter(root / "stderr.log")
            child_script = 'import sys,time; sys.stdout.write("child\\n"); sys.stdout.flush(); time.sleep(30)'
            child = f"{shlex.quote(sys.executable)} -c {shlex.quote(child_script)}"
            process = subprocess.Popen(
                f"{child} & exit 0",
                shell=True,
                start_new_session=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            self.assertIsNotNone(process.stdout)
            self.assertIsNotNone(process.stderr)
            output_threads = [
                threading.Thread(
                    target=HARNESS.drain_process_output,
                    args=(process.stdout, stdout_log),
                    daemon=True,
                ),
                threading.Thread(
                    target=HARNESS.drain_process_output,
                    args=(process.stderr, stderr_log),
                    daemon=True,
                ),
            ]
            for thread in output_threads:
                thread.start()

            try:
                deadline = time.monotonic() + 2
                while process.poll() is None and time.monotonic() < deadline:
                    time.sleep(0.01)
                self.assertIsNotNone(process.poll())
                cleanup = HARNESS.terminate_process(process, output_threads)

                self.assertEqual(cleanup["status"], "validated")
                self.assertEqual(cleanup["remaining_pids"], [])
                self.assertTrue(cleanup["output_threads_joined"])
                self.assertTrue(all(not thread.is_alive() for thread in output_threads))
                stdout_log.close()
                stderr_log.close()
            finally:
                if process.poll() is None:
                    try:
                        os.killpg(process.pid, 9)
                    except ProcessLookupError:
                        pass
                for thread in output_threads:
                    thread.join(timeout=2)
                if not stdout_log.evidence()["closed"]:
                    stdout_log.close()
                if not stderr_log.evidence()["closed"]:
                    stderr_log.close()

    def test_archive_revision_reaps_archive_child_when_extraction_fails(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source, revision = create_test_git_repo(Path(temporary))
            destination = Path(temporary) / "archive"
            destination.mkdir()
            spawned: list[subprocess.Popen[bytes]] = []
            real_popen = HARNESS.subprocess.Popen

            def fake_archive_popen(command: object, **kwargs: object) -> subprocess.Popen[bytes]:
                if not isinstance(command, list) or "archive" not in command:
                    return real_popen(command, **kwargs)
                process = real_popen(
                    [sys.executable, "-c", "import time; time.sleep(30)"],
                    **kwargs,
                )
                spawned.append(process)
                return process

            with mock.patch.object(
                HARNESS.subprocess,
                "Popen",
                side_effect=fake_archive_popen,
            ):
                with mock.patch.object(
                    HARNESS.tarfile,
                    "open",
                    side_effect=OSError(errno.ENOSPC, "No space left on device"),
                ):
                    with self.assertRaises(OSError) as raised:
                        HARNESS.archive_revision(source, revision, destination)

            self.assertEqual(len(spawned), 1)
            archive_process = spawned[0]
            self.assertIsNotNone(archive_process.poll())
            self.assertIsNotNone(archive_process.returncode)
            self.assertIsNotNone(archive_process.stdout)
            self.assertTrue(archive_process.stdout.closed)
            self.assertIn("OSError", HARNESS.exception_evidence(raised.exception)["traceback"])

    def test_archive_revision_python311_fallback_is_safe(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source, revision = create_test_git_repo(Path(temporary))
            destination = Path(temporary) / "archive"
            destination.mkdir()

            with mock.patch.object(HARNESS.sys, "version_info", (3, 11)):
                resolved = HARNESS.archive_revision(source, revision, destination)

            self.assertEqual(resolved, revision)
            self.assertEqual(
                (destination / "fixture.txt").read_text(encoding="ascii"),
                "synthetic fixture\n",
            )

            unsafe_members = (
                ("absolute", "/absolute.txt", HARNESS.tarfile.REGTYPE, ""),
                ("traversal", "../escape.txt", HARNESS.tarfile.REGTYPE, ""),
                ("symlink", "link", HARNESS.tarfile.SYMTYPE, "../escape.txt"),
                ("hardlink", "hardlink", HARNESS.tarfile.LNKTYPE, "fixture.txt"),
                ("fifo", "fifo", HARNESS.tarfile.FIFOTYPE, ""),
            )
            for label, name, member_type, linkname in unsafe_members:
                with self.subTest(label=label):
                    archive_bytes = io.BytesIO()
                    with HARNESS.tarfile.open(fileobj=archive_bytes, mode="w") as stream:
                        member = HARNESS.tarfile.TarInfo(name)
                        member.type = member_type
                        member.linkname = linkname
                        if member.isfile():
                            payload = b"unsafe"
                            member.size = len(payload)
                            stream.addfile(member, io.BytesIO(payload))
                        else:
                            stream.addfile(member)
                    archive_bytes.seek(0)
                    unsafe_destination = Path(temporary) / f"unsafe-{label}"
                    unsafe_destination.mkdir()

                    with mock.patch.object(HARNESS.sys, "version_info", (3, 11)):
                        with mock.patch.object(
                            HARNESS.tarfile.TarFile,
                            "extractall",
                            side_effect=AssertionError("Python 3.11 fallback used extractall"),
                        ):
                            with HARNESS.tarfile.open(
                                fileobj=archive_bytes, mode="r|"
                            ) as stream:
                                with self.assertRaises(ValueError):
                                    HARNESS.extract_archive_safely(stream, unsafe_destination)

                    self.assertFalse((Path(temporary) / "escape.txt").exists())

    def test_run_variant_preserves_archive_failure_stage_and_workspace(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source, revision = create_test_git_repo(Path(temporary))
            args = type(
                "Args",
                (),
                {
                    "subjects": tuple(f"subject-{index}" for index in range(1, 15)),
                    "tokens": tuple(f"token-{index}" for index in range(1, 15)),
                    "request_interval_seconds": 0.25,
                    "bootstrap_bound_business_tokens": False,
                    "resources": HARNESS.DEFAULT_RESOURCES,
                    "resource_spec": HARNESS.parse_resource_spec(HARNESS.DEFAULT_RESOURCES),
                    "service_command": None,
                    "binary_manifest": None,
                },
            )()
            workdir = Path(temporary) / "run" / ".work" / "initial" / "round-1" / "candidate"
            log_directory = Path(temporary) / "logs"
            error = OSError(errno.ENOSPC, "No space left on device", str(workdir / "repo"))
            with mock.patch.object(HARNESS, "archive_revision", side_effect=error):
                with HARNESS.MockUpstream(HARNESS.DEFAULT_MOCK_DELAY_MS) as mock_upstream:
                    result = HARNESS.run_variant(
                        source,
                        revision,
                        "candidate",
                        workdir,
                        mock_upstream,
                        args,
                        1,
                        1,
                        log_directory,
                    )

            self.assertEqual(result["reason"], "variant_exception")
            self.assertEqual(result["failure_stage"], "archive_revision")
            self.assertEqual(result["error"]["errno"], errno.ENOSPC)
            self.assertIn("OSError", result["error"]["traceback"])
            self.assertEqual(result["cleanup"]["process"]["status"], "validated")
            self.assertEqual(result["workspace"]["cleanup"]["status"], "preserved")
            self.assertTrue(workdir.exists())

    def test_real_paired_rounds_use_and_release_variant_workspaces(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source, revision = create_test_git_repo(Path(temporary))
            run_directory = Path(temporary) / "run"
            args = type(
                "Args",
                (),
                {
                    "rounds": 1,
                    "baseline_ref": revision,
                    "candidate_ref": revision,
                    "min_samples": 1,
                    "min_control_samples": 1,
                    "subjects": tuple(f"subject-{index}" for index in range(1, 15)),
                    "tokens": tuple(f"token-{index}" for index in range(1, 15)),
                    "request_interval_seconds": 0.25,
                    "bootstrap_bound_business_tokens": False,
                    "bootstrap_business_tokens": False,
                    "resources": HARNESS.DEFAULT_RESOURCES,
                    "resource_spec": HARNESS.parse_resource_spec(HARNESS.DEFAULT_RESOURCES),
                    "service_command": None,
                    "binary_manifest": None,
                },
            )()
            checkpoint = HARNESS.CheckpointStore(
                run_directory, {"kind": "test", "scenarios": SCENARIOS}
            )
            with HARNESS.MockUpstream(HARNESS.DEFAULT_MOCK_DELAY_MS) as mock_upstream:
                rounds = HARNESS.run_paired_rounds(
                    source,
                    run_directory,
                    mock_upstream,
                    args,
                    1,
                    "initial",
                    checkpoint,
                )

            self.assertEqual(len(rounds), 1)
            for variant in ("baseline", "candidate"):
                workspace = HARNESS.variant_workspace_path(
                    run_directory, "initial", 1, variant
                )
                checkpoint_path = (
                    run_directory / "variants" / f"initial-round-1-{variant}.json"
                )
                result = json.loads(checkpoint_path.read_text(encoding="utf-8"))
                self.assertEqual(result["workspace"]["path"], str(workspace.resolve()))
                self.assertEqual(result["workspace"]["cleanup"]["status"], "released")
                self.assertFalse(workspace.exists())

    def test_cleanup_unvalidated_from_real_run_variant_blocks_next_candidate(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, revision = create_test_git_repo(root)
            service = root / "service.py"
            service.write_text(
                "#!/usr/bin/env python3\nimport time\ntime.sleep(30)\n",
                encoding="ascii",
            )
            service.chmod(0o755)
            manifest = root / "manifest.json"
            manifest.write_text(
                json.dumps(
                    {
                        "source_sha": revision,
                        "build_input_sha": revision,
                        "binary": {
                            "path": str(service),
                            "sha256": HARNESS.sha256_file(service),
                        },
                        "build": {
                            "compiler": "test",
                            "profile": "debug",
                            "flags": [],
                        },
                    }
                ),
                encoding="ascii",
            )
            args = type(
                "Args",
                (),
                {
                    "rounds": 2,
                    "baseline_ref": revision,
                    "candidate_ref": revision,
                    "min_samples": 1,
                    "min_control_samples": 1,
                    "subjects": tuple(f"subject-{index}" for index in range(1, 15)),
                    "tokens": tuple(f"token-{index}" for index in range(1, 15)),
                    "request_interval_seconds": 0.25,
                    "bootstrap_bound_business_tokens": False,
                    "bootstrap_business_tokens": False,
                    "resources": HARNESS.DEFAULT_RESOURCES,
                    "resource_spec": HARNESS.parse_resource_spec(HARNESS.DEFAULT_RESOURCES),
                    "service_command": "{binary}",
                    "binary_manifest": str(manifest),
                    "metrics_enabled": True,
                    "metrics_scrape_interval_seconds": 0.0,
                    "readiness_timeout_seconds": 0.0,
                },
            )()
            run_directory = root / "run"
            checkpoint = HARNESS.CheckpointStore(
                run_directory, {"kind": "test", "scenarios": SCENARIOS}
            )
            real_terminate = HARNESS.terminate_process

            def report_unvalidated_cleanup(
                process: subprocess.Popen[bytes],
                output_threads: object = (),
                process_group_id: int | None = None,
            ) -> dict[str, object]:
                cleanup = real_terminate(process, output_threads, process_group_id)
                cleanup["status"] = "unvalidated"
                cleanup["output_threads_joined"] = False
                cleanup["errors"] = [
                    {"stage": "output_thread_cleanup", "reason": "test_drain_not_joined"}
                ]
                return cleanup

            with warnings.catch_warnings():
                warnings.simplefilter("ignore", ResourceWarning)
                with mock.patch.object(
                    HARNESS,
                    "apply_resource_constraints",
                    return_value=RESOURCES,
                ), mock.patch.object(
                    HARNESS,
                    "terminate_process",
                    side_effect=report_unvalidated_cleanup,
                ), HARNESS.MockUpstream(HARNESS.DEFAULT_MOCK_DELAY_MS) as mock_upstream:
                    rounds = HARNESS.run_paired_rounds(
                        source,
                        run_directory,
                        mock_upstream,
                        args,
                        1,
                        "initial",
                        checkpoint,
                    )

            baseline = rounds[0]["baseline"]
            candidate = rounds[0]["candidate"]
            self.assertEqual(baseline["cleanup"]["status"], "unvalidated")
            self.assertEqual(baseline["cleanup"]["process"]["output_threads_joined"], False)
            self.assertEqual(baseline["cleanup"]["process"]["remaining_pids"], [])
            self.assertEqual(candidate["reason"], "prior_cleanup_unvalidated")
            self.assertEqual(candidate["cleanup"]["status"], "not_run")
            self.assertIsNone(candidate["request"])
            self.assertEqual(candidate["system_diagnostics"]["status"], "unavailable")
            self.assertIsNone(candidate["resource_observation"])
            candidate_workspace = HARNESS.variant_workspace_path(
                run_directory, "initial", 1, "candidate"
            )
            self.assertFalse(candidate_workspace.exists())

    def test_variant_exception_preserves_stage_errno_traceback_and_bound_metadata(self) -> None:
        args = type(
            "Args",
            (),
            {
                "bootstrap_bound_business_tokens": True,
                "tokens": tuple(f"token-{index}" for index in range(1, 15)),
                "subjects": tuple(f"subject-{index}" for index in range(1, 15)),
                "request_interval_seconds": 0.25,
            },
        )()
        error = OSError(errno.ENOSPC, "No space left on device", "/tmp/candidate-repo")

        try:
            raise error
        except OSError as caught:
            result = HARNESS.variant_exception_result(
                "candidate",
                3,
                "candidate-ref",
                args,
                caught,
                stage="archive_revision",
            )

        self.assertEqual(result["reason"], "variant_exception")
        self.assertEqual(result["error_type"], "OSError")
        self.assertEqual(result["error"]["errno"], errno.ENOSPC)
        self.assertEqual(result["error"]["strerror"], "No space left on device")
        self.assertEqual(result["error"]["filename"], "/tmp/candidate-repo")
        self.assertEqual(result["error"]["stage"], "archive_revision")
        self.assertIn("OSError", result["error"]["traceback"])
        mapping = result["credential_mapping"]
        self.assertEqual(mapping["credential_kind"], "bound_access_token")
        self.assertEqual(mapping["billing_subject_kind"], "account")
        self.assertEqual(mapping["binding_path"], "POST /api/users/<account-id>/tokens")
        self.assertEqual(len(mapping["account_ids"]), 14)

    def test_run_directory_workspace_records_storage_and_releases_variant_workspace(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            run_directory = Path(temporary) / "run"
            workspace = HARNESS.variant_workspace_path(
                run_directory, "initial", 3, "candidate"
            )
            workspace.mkdir(parents=True)
            (workspace / "partial-archive").write_bytes(b"synthetic")

            storage = HARNESS.workspace_storage_evidence(workspace)
            self.assertEqual(storage["path"], str(workspace))
            self.assertGreater(storage["available_bytes"], 0)
            self.assertGreater(storage["available_inodes"], 0)
            self.assertIn("mount", storage)
            self.assertEqual(
                storage["tempdir"],
                HARNESS.temp_directory_evidence()["path"],
            )

            released = HARNESS.release_variant_workspace(workspace, cleanup_validated=True)

            self.assertEqual(released["status"], "released")
            self.assertFalse(workspace.exists())
            self.assertEqual(released["after"]["exists"], False)

    def test_failure_checkpoint_preserves_error_cleanup_and_workspace_evidence(self) -> None:
        args = type(
            "Args",
            (),
            {
                "bootstrap_bound_business_tokens": False,
                "tokens": tuple(f"token-{index}" for index in range(1, 15)),
                "subjects": tuple(f"subject-{index}" for index in range(1, 15)),
                "request_interval_seconds": 0.25,
            },
        )()
        try:
            raise OSError(errno.ENOSPC, "No space left on device", "/tmp/archive")
        except OSError as error:
            result = HARNESS.variant_exception_result(
                "candidate",
                1,
                "candidate-ref",
                args,
                error,
                stage="archive_revision",
            )
        result["cleanup"] = {"status": "unvalidated", "reason": "process_cleanup_unvalidated"}
        result["workspace"] = {"cleanup": {"status": "preserved"}}

        with tempfile.TemporaryDirectory() as temporary:
            checkpoint = HARNESS.CheckpointStore(Path(temporary) / "run", {"kind": "test"})
            checkpoint.write_variant_result("initial", 1, "candidate", result)
            payload = json.loads(
                (Path(temporary) / "run" / "variants" / "initial-round-1-candidate.json").read_text(
                    encoding="utf-8"
                )
            )

        self.assertEqual(payload["error"]["errno"], errno.ENOSPC)
        self.assertEqual(payload["failure_stage"], "archive_revision")
        self.assertEqual(payload["cleanup"]["status"], "unvalidated")
        self.assertEqual(payload["workspace"]["cleanup"]["status"], "preserved")

    def test_paired_rounds_isolate_workspaces_and_stop_after_unvalidated_cleanup(self) -> None:
        args = type(
            "Args",
            (),
            {
                "rounds": 3,
                "baseline_ref": "baseline-ref",
                "candidate_ref": "candidate-ref",
                "min_samples": 1,
                "min_control_samples": 1,
                "subjects": tuple(f"subject-{index}" for index in range(1, 15)),
                "tokens": tuple(f"token-{index}" for index in range(1, 15)),
                "request_interval_seconds": 0.25,
                "bootstrap_bound_business_tokens": False,
            },
        )()
        baseline = measured_result(
            variant="baseline", round_number=1, request_p95=100, unrelated_p95=100
        )
        baseline["cleanup"] = {
            "status": "unvalidated",
            "reason": "process_cleanup_unvalidated",
        }

        with tempfile.TemporaryDirectory() as temporary:
            checkpoint = HARNESS.CheckpointStore(Path(temporary) / "run", {"kind": "test"})
            with mock.patch.object(HARNESS, "run_variant", return_value=baseline) as run:
                rounds = HARNESS.run_paired_rounds(
                    ROOT,
                    Path(temporary) / "run",
                    object(),
                    args,
                    1,
                    "initial",
                    checkpoint,
                )

            candidate_path = (
                Path(temporary)
                / "run"
                / "variants"
                / "initial-round-1-candidate.json"
            )
            candidate = json.loads(candidate_path.read_text(encoding="utf-8"))

        self.assertEqual(run.call_count, 1)
        self.assertEqual(
            run.call_args.args[3],
            HARNESS.variant_workspace_path(
                Path(temporary) / "run", "initial", 1, "baseline"
            ),
        )
        self.assertEqual(len(rounds), 1)
        self.assertEqual(rounds[0]["candidate"]["reason"], "prior_cleanup_unvalidated")
        self.assertEqual(candidate["blocked_by"], "baseline")
        self.assertEqual(candidate["cleanup"]["status"], "not_run")

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

    def test_service_process_binding_selects_manifest_matching_descendant(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            proc_root = root / "proc"
            binary = root / "service"
            launcher = root / "launcher"
            binary.write_bytes(b"synthetic-service")
            launcher.write_bytes(b"synthetic-launcher")
            for pid in (41, 42):
                task_root = proc_root / str(pid) / "task" / str(pid)
                task_root.mkdir(parents=True)
                (task_root / "children").write_text(
                    "42\n" if pid == 41 else "", encoding="ascii"
                )
            (proc_root / "41" / "exe").symlink_to(launcher)
            (proc_root / "42" / "exe").symlink_to(binary)
            provenance = {
                "status": "verified",
                "binary_path": str(binary),
                "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
            }

            evidence = HARNESS.service_process_binary_binding(41, provenance, proc_root)

            self.assertEqual(evidence["status"], "verified")
            self.assertEqual(evidence["launcher_pid"], 41)
            self.assertEqual(evidence["service_pid"], 42)
            self.assertEqual(evidence["service_exe"], str(binary))
            self.assertEqual(evidence["service_sha256"], provenance["binary_sha256"])
            launcher_candidate = next(
                candidate
                for candidate in evidence["candidate_processes"]
                if candidate["pid"] == 41
            )
            self.assertFalse(launcher_candidate["matches_manifest"])
            unavailable = HARNESS.service_process_binary_binding(
                41,
                {**provenance, "binary_sha256": "0" * 64},
                proc_root,
            )
            self.assertEqual(unavailable["status"], "unavailable")
            self.assertEqual(unavailable["reason"], "service_binary_process_not_found")

            replaced_path = root / "service-replaced"
            replaced_path.write_bytes(b"replacement-at-the-same-path")
            (proc_root / "42" / "exe").unlink()
            (proc_root / "42" / "exe").symlink_to(replaced_path)

            def hash_proc_executable(path: Path) -> str:
                if path == proc_root / "42" / "exe":
                    return provenance["binary_sha256"]
                return hashlib.sha256(path.read_bytes()).hexdigest()

            with mock.patch.object(HARNESS, "sha256_file", side_effect=hash_proc_executable):
                replaced_evidence = HARNESS.service_process_binary_binding(
                    41, provenance, proc_root
                )
            self.assertEqual(replaced_evidence["status"], "verified")
            self.assertEqual(replaced_evidence["service_pid"], 42)
            self.assertEqual(replaced_evidence["service_exe"], str(replaced_path))

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

    def test_system_diagnostics_blocked_capture_is_unavailable_until_joined(self) -> None:
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

            original_collect = HARNESS.collect_system_snapshot
            capture_count = 0
            capture_entered = threading.Event()
            release_capture = threading.Event()

            def controlled_capture(pid: int, proc_root: Path) -> dict[str, object]:
                nonlocal capture_count
                capture_count += 1
                if capture_count == 3:
                    capture_entered.set()
                    release_capture.wait(timeout=2)
                return original_collect(pid, proc_root)

            collector = HARNESS.SystemDiagnosticsCollector(
                42, interval_seconds=0.01, proc_root=proc_root
            )
            with mock.patch.object(
                HARNESS, "collect_system_snapshot", side_effect=controlled_capture
            ):
                collector.start()
                try:
                    self.assertTrue(capture_entered.wait(timeout=2))
                    collector.stop()
                    evidence = collector.evidence()
                    self.assertEqual(evidence["status"], "unavailable")
                    self.assertEqual(evidence["reason"], "collector_join_timeout")
                    self.assertIn("collector_join_timeout", evidence["errors"])
                    self.assertFalse(evidence["joined"])
                    self.assertIsNotNone(collector._thread)
                    self.assertTrue(collector._thread.is_alive())
                finally:
                    release_capture.set()
                    collector.stop()
                    thread = collector._thread
                    if thread is not None:
                        thread.join(timeout=2)
                    collector.stop()

            self.assertIsNone(collector._thread)

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
                "service_process",
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
