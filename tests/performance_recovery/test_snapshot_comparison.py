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


if __name__ == "__main__":
    unittest.main()
