#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import pathlib
import tempfile
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


class AlertsCoverageTests(unittest.TestCase):
    def test_recorder_requires_success_after_restart_for_each_route(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            marker = pathlib.Path(directory) / "restart.marker"
            begin = pathlib.Path(directory) / "restart.begin"
            recorder = LOAD.Recorder(marker, begin)
            recorder.alert_first_success_secs = {route: 1.0 for route in LOAD.ALERT_ROUTES}
            begin.touch()
            marker.touch()
            for route in LOAD.ALERT_ROUTES:
                recorder.attempt(f"alerts_{route}")
                recorder.status(f"alerts_{route}", 200, 0.0, alert_fresh=True)

        self.assertEqual(
            recorder.alert_post_restart_successes,
            {route: 1 for route in LOAD.ALERT_ROUTES},
        )
        self.assertEqual(recorder.alert_post_restart_failures, {})

    def test_recorder_does_not_count_expected_restart_gap_as_post_warm_failure(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            marker = pathlib.Path(directory) / "restart.marker"
            begin = pathlib.Path(directory) / "restart.begin"
            recorder = LOAD.Recorder(marker, begin)
            recorder.alert_first_success_secs["catalog"] = 1.0
            begin.touch()
            recorder.error("alerts_catalog", OSError("connection reset"))

        self.assertEqual(recorder.alert_post_warm_failures, {})
        self.assertEqual(recorder.alert_post_restart_failures, {})

    def test_recorder_counts_post_restart_non_200_as_a_failure(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            marker = pathlib.Path(directory) / "restart.marker"
            begin = pathlib.Path(directory) / "restart.begin"
            recorder = LOAD.Recorder(marker, begin)
            recorder.alert_first_success_secs["catalog"] = 1.0
            begin.touch()
            marker.touch()
            recorder.status("alerts_catalog", 404, 0.0)

        self.assertEqual(recorder.alert_post_warm_failures["catalog"], 1)
        self.assertEqual(recorder.alert_post_restart_failures["catalog"], 1)

    def test_recorder_rejects_stale_200_as_alert_success(self) -> None:
        recorder = LOAD.Recorder()
        recorder.status("alerts_catalog", 200, 0.0, alert_fresh=False)

        self.assertEqual(recorder.alert_first_success_secs, {})
        self.assertEqual(recorder.alert_non_fresh_200["catalog"], 1)

    def test_alert_payload_accepts_fresh_shape_and_rejects_stale_shape(self) -> None:
        self.assertTrue(LOAD.alerts_payload_is_fresh(200, b'{"items":[]}'))
        self.assertTrue(LOAD.alerts_payload_is_fresh(200, b'{"coverage":"fresh"}'))
        self.assertFalse(LOAD.alerts_payload_is_fresh(200, b'{"coverage":"stale"}'))
        self.assertFalse(LOAD.alerts_payload_is_fresh(200, b"not-json"))


if __name__ == "__main__":
    unittest.main()
