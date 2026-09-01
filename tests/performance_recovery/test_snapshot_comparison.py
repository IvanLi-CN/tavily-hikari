#!/usr/bin/env python3
"""Keep the production-shape GC proof aligned with runtime retention semantics."""

from __future__ import annotations

import pathlib
import re
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]
HA_DEFS = (ROOT / "src/store/key_store_ha_defs.rs").read_text()
COMPARISON = (ROOT / "tests/performance_recovery/run_snapshot_comparison.sh").read_text()
DOCKERFILE = (ROOT / "Dockerfile").read_text()
DOCKERIGNORE = (ROOT / ".dockerignore").read_text()


def rust_resources(constant: str) -> set[str]:
    match = re.search(
        rf"const {constant}:.*?=\s*&\[(?P<items>.*?)\];",
        HA_DEFS,
        flags=re.DOTALL,
    )
    if match is None:
        raise AssertionError(f"missing {constant}")
    return set(re.findall(r'"([^"]+)"', match.group("items")))


def comparison_resources(variable: str) -> set[str]:
    match = re.search(rf'^%s="(?P<items>[^"]+)"$' % variable, COMPARISON, flags=re.MULTILINE)
    if match is None:
        raise AssertionError(f"missing {variable}")
    return set(re.findall(r"'([^']+)'", match.group("items")))


class SnapshotComparisonTests(unittest.TestCase):
    def test_gc_debt_gate_matches_runtime_allowed_resources(self) -> None:
        expected = {
            "HA_GC_CONTROL_RESOURCES": rust_resources("HA_CONTROL_EVENT_TABLES"),
            "HA_GC_BILLING_RESOURCES": rust_resources("HA_BILLING_BASELINE_TABLES"),
            "HA_GC_RUNTIME_RESOURCES": rust_resources("HA_RUNTIME_EVENT_TABLES"),
        }

        for variable, resources in expected.items():
            with self.subTest(variable=variable):
                self.assertSetEqual(comparison_resources(variable), resources)

    def test_comparison_keeps_calibrated_noise_bounds_and_raw_metrics(self) -> None:
        self.assertIn("DASHBOARD_P95_NOISE_FLOOR_MS = 15.0", COMPARISON)
        self.assertIn("RSS_P95_NOISE_BAND_KIB = 40 * 1024", COMPARISON)
        self.assertIn("absolute_floor=DASHBOARD_P95_NOISE_FLOOR_MS", COMPARISON)
        self.assertIn("additive_tolerance=RSS_P95_NOISE_BAND_KIB", COMPARISON)
        self.assertIn('return summary["load"]["dashboardP95Ms"]', COMPARISON)
        self.assertIn('"rssP95KiB": p95("rss_kib")', COMPARISON)
        self.assertIn('"foregroundHttp5xx": lane_5xx("business")', COMPARISON)
        self.assertIn('"dashboardHttp5xx": lane_5xx("dashboard")', COMPARISON)
        self.assertIn('"maintenanceHttp5xx": lane_5xx("ha_gc_trigger")', COMPARISON)
        self.assertIn('"sqliteTransientLockRetries": sqlite_transient_lock_retries', COMPARISON)
        self.assertIn('"sqliteTypedLockDeferrals": sqlite_typed_lock_deferrals', COMPARISON)
        self.assertIn('"sqliteFinalLockErrors": sqlite_final_lock_errors', COMPARISON)
        self.assertIn('candidate["sqliteFinalLockErrors"]', COMPARISON)
        self.assertIn('structured_field(line, "defer_reason", reason)', COMPARISON)
        self.assertIn('("sqlite_contention", "sqlite_busy")', COMPARISON)
        self.assertIn('structured_field(line, "event", "research_sweep_deferred")', COMPARISON)
        self.assertIn('structured_field(line, "reason", "local_pressure")', COMPARISON)
        self.assertIn("baseline_business_response_ratio", COMPARISON)
        self.assertIn("baseline_business_response_ratio - 0.05", COMPARISON)
        self.assertIn("business_attempts * candidate_response_ratio_floor", COMPARISON)
        self.assertIn('"database table is locked"', COMPARISON)
        self.assertIn('"database schema is locked"', COMPARISON)
        self.assertIn('"database is busy"', COMPARISON)
        self.assertIn('"terminalDelta": reconciliation_after["terminal"]', COMPARISON)
        self.assertIn('"reconciliationProjectionDiscarded"', COMPARISON)
        self.assertIn('candidate reconciliation produced no terminal outcome', COMPARISON)
        self.assertIn('candidate did not complete the deterministic shadow reconciliation fixture', COMPARISON)
        self.assertIn('candidate Research drain produced no terminal outcome', COMPARISON)
        self.assertIn('candidate Research pending backlog grew during the comparison', COMPARISON)
        self.assertIn('candidate did not complete the deterministic Research drain fixture', COMPARISON)
        self.assertIn('"fixtureResearchTerminal"', COMPARISON)
        self.assertIn('"researchTerminalDelta"', COMPARISON)
        self.assertIn('"researchPendingDelta"', COMPARISON)
        self.assertIn('"transient sqlite error" in line and "attempt=" in line', COMPARISON)
        self.assertIn('projection transaction p95 is not proven below 100ms', COMPARISON)
        self.assertIn('candidate billing truth differs', COMPARISON)
        self.assertIn("prepare_reconciliation_fixture", COMPARISON)
        self.assertIn("Historical baselines predate this Dockerfile input allowlist", COMPARISON)
        self.assertIn("grep -qx '!rust-toolchain.toml'", COMPARISON)
        self.assertIn("completed_generation < work_generation", COMPARISON)
        self.assertIn("upstream_reconciliation_backoff_until_v1", COMPARISON)
        self.assertIn("snapshot forward-proxy transport isolation failed", COMPARISON)
        self.assertIn("subscription_urls_json = '[]'", COMPARISON)
        self.assertIn("egress_socks5_enabled = 0", COMPARISON)
        self.assertIn("testbox-reconciliation-shadow-token", COMPARISON)
        self.assertIn("testbox-reconciliation-shadow-period", COMPARISON)
        self.assertIn("tvly-reconciliation-fixture-key", COMPARISON)
        self.assertIn("api_key_low_quota_depletions", COMPARISON)
        self.assertIn("API rebalance excludes it from foreground selection", COMPARISON)
        self.assertIn("Reconciliation fetches its persisted secret directly", COMPARISON)
        self.assertIn(
            "TAVILY_API_KEYS: tvly-load-key,tvly-reconciliation-fixture-key",
            COMPARISON,
        )
        self.assertIn("snapshot reconciliation fixture preparation failed", COMPARISON)
        self.assertIn("testbox-reconciliation-research-request", COMPARISON)
        self.assertIn("DELETE FROM api_key_transient_backoffs", COMPARISON)
        self.assertIn("upstream_reconciliation_research_scan_state", COMPARISON)
        self.assertIn("upstream_reconciliation_control_state", COMPARISON)
        self.assertIn("the persisted legacy switch above produces compare mode", COMPARISON)

    def test_docker_context_allows_the_test_toolchain_input(self) -> None:
        self.assertIn("!rust-toolchain.toml", DOCKERIGNORE)
        self.assertIn("build.rs|rust-toolchain.toml|src", DOCKERFILE)


if __name__ == "__main__":
    unittest.main()
