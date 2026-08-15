#!/usr/bin/env python3
"""Keep the production-shape GC proof aligned with runtime retention semantics."""

from __future__ import annotations

import pathlib
import re
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]
HA_DEFS = (ROOT / "src/store/key_store_ha_defs.rs").read_text()
COMPARISON = (ROOT / "tests/performance_recovery/run_snapshot_comparison.sh").read_text()


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
        self.assertIn("DASHBOARD_P95_NOISE_FLOOR_MS = 10.0", COMPARISON)
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
        self.assertIn('(baseline_business_responses * 95 + 99) // 100', COMPARISON)
        self.assertIn('"database table is locked"', COMPARISON)
        self.assertIn('"database schema is locked"', COMPARISON)
        self.assertIn('"database is busy"', COMPARISON)


if __name__ == "__main__":
    unittest.main()
