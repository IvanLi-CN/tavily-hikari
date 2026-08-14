#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import pathlib
import unittest


LOAD_PATH = pathlib.Path(__file__).with_name("load.py")
SPEC = importlib.util.spec_from_file_location("performance_recovery_load", LOAD_PATH)
assert SPEC is not None and SPEC.loader is not None
LOAD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(LOAD)


class PeriodicScheduleTests(unittest.TestCase):
    def test_slow_action_skips_missed_intervals(self) -> None:
        deadline = LOAD.next_periodic_deadline(
            previous_deadline=100.0,
            interval_secs=1.0,
            now=110.0,
        )

        self.assertEqual(deadline, 111.0)

    def test_on_time_action_preserves_fixed_rate(self) -> None:
        deadline = LOAD.next_periodic_deadline(
            previous_deadline=100.0,
            interval_secs=10.0,
            now=105.0,
        )

        self.assertEqual(deadline, 110.0)


class RecoveryTailTests(unittest.TestCase):
    def test_full_production_shape_reserves_a_quiet_gc_tail(self) -> None:
        self.assertEqual(LOAD.recovery_tail_secs_for_duration(600, None), 60)

    def test_short_diagnostic_keeps_its_entire_traffic_window(self) -> None:
        self.assertEqual(LOAD.recovery_tail_secs_for_duration(60, None), 0)

    def test_recovery_tail_must_fit_inside_the_total_duration(self) -> None:
        with self.assertRaises(ValueError):
            LOAD.recovery_tail_secs_for_duration(60, 60)


if __name__ == "__main__":
    unittest.main()
