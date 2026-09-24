#!/usr/bin/env python3
"""Internal-only production-shape traffic for the recovery comparison."""

from __future__ import annotations

import argparse
import http.client
import json
import threading
import time
from collections import Counter
from pathlib import Path


DASHBOARD_CLIENTS = 20
DASHBOARD_INTERVAL_SECS = 60.0
ALERT_ROUTES = ("catalog", "events", "groups")
ALERT_INTERVAL_SECS = 5.0
BUSINESS_CLIENTS = 5
BUSINESS_INTERVAL_SECS = 1.0
# A production-shaped snapshot may have bounded startup maintenance reclaiming
# the three SQLite connections. Bootstrap traffic is outside the measured load
# lane, so give both variants the same finite window to become ready.
BOOTSTRAP_DEADLINE_SECS = 180.0


class Recorder:
    def __init__(
        self,
        restart_marker: Path | None = None,
        restart_begin_marker: Path | None = None,
    ) -> None:
        self._lock = threading.Lock()
        self.restart_marker = restart_marker
        self.restart_begin_marker = restart_begin_marker
        self.restart_started_at: float | None = None
        self.restart_observed_at: float | None = None
        self.dashboard_ms: list[float] = []
        self.dashboard_attempts = 0
        self.business_attempts = 0
        self.statuses: Counter[str] = Counter()
        self.errors: Counter[str] = Counter()
        self.events: Counter[str] = Counter()
        self.alert_attempts: Counter[str] = Counter()
        self.alert_post_restart_attempts: Counter[str] = Counter()
        self.alert_post_restart_successes: Counter[str] = Counter()
        self.alert_post_restart_failures: Counter[str] = Counter()
        self.alert_first_success_secs: dict[str, float] = {}
        self.alert_non_fresh_200: Counter[str] = Counter()
        self.alert_post_warm_5xx: Counter[str] = Counter()
        self.alert_post_warm_failures: Counter[str] = Counter()
        self.started_at = time.monotonic()

    def observe_restart(self) -> None:
        if self.restart_marker is None and self.restart_begin_marker is None:
            return
        with self._lock:
            if (
                self.restart_started_at is None
                and self.restart_begin_marker is not None
                and self.restart_begin_marker.exists()
            ):
                self.restart_started_at = time.monotonic()
            if self.restart_observed_at is None:
                if self.restart_marker is not None and self.restart_marker.exists():
                    self.restart_observed_at = time.monotonic()

    def restart_in_progress(self) -> bool:
        return self.restart_started_at is not None and self.restart_observed_at is None

    def attempt(self, lane: str) -> None:
        self.observe_restart()
        with self._lock:
            if lane == "dashboard":
                self.dashboard_attempts += 1
            elif lane == "business":
                self.business_attempts += 1
            if lane.startswith("alerts_"):
                route = lane.removeprefix("alerts_")
                self.alert_attempts[route] += 1
                if self.restart_observed_at is not None:
                    self.alert_post_restart_attempts[route] += 1

    def _record_alert_failure_locked(self, route: str, post_restart: bool) -> None:
        if route in self.alert_first_success_secs and not self.restart_in_progress():
            self.alert_post_warm_failures[route] += 1
        if post_restart:
            self.alert_post_restart_failures[route] += 1

    def mark_started(self) -> None:
        with self._lock:
            self.started_at = time.monotonic()

    def status(
        self,
        lane: str,
        status: int,
        elapsed_ms: float,
        alert_fresh: bool | None = None,
    ) -> None:
        self.observe_restart()
        with self._lock:
            self.statuses[f"{lane}:{status}"] += 1
            if lane == "dashboard":
                self.dashboard_ms.append(elapsed_ms)
            if lane.startswith("alerts_"):
                route = lane.removeprefix("alerts_")
                elapsed_secs = time.monotonic() - self.started_at
                post_restart = self.restart_observed_at is not None
                if status == 200 and alert_fresh is True:
                    self.alert_first_success_secs.setdefault(route, elapsed_secs)
                    if post_restart:
                        self.alert_post_restart_successes[route] += 1
                else:
                    if status == 200:
                        self.alert_non_fresh_200[route] += 1
                    self._record_alert_failure_locked(route, post_restart)
                    if status >= 500 and route in self.alert_first_success_secs:
                        self.alert_post_warm_5xx[route] += 1

    def error(self, lane: str, error: BaseException) -> None:
        self.observe_restart()
        with self._lock:
            self.errors[f"{lane}:{type(error).__name__}"] += 1
            if lane.startswith("alerts_"):
                route = lane.removeprefix("alerts_")
                self._record_alert_failure_locked(
                    route,
                    self.restart_observed_at is not None,
                )

    def event(self, lane: str) -> None:
        with self._lock:
            self.events[lane] += 1

    def summary(self) -> dict[str, object]:
        with self._lock:
            ordered = sorted(self.dashboard_ms)
            p95 = (
                ordered[min(len(ordered) - 1, int(len(ordered) * 0.95))]
                if ordered
                else None
            )
            return {
                "dashboardAttempts": self.dashboard_attempts,
                "businessAttempts": self.business_attempts,
                "businessClients": BUSINESS_CLIENTS,
                "businessIntervalSecs": BUSINESS_INTERVAL_SECS,
                "dashboardRequests": len(ordered),
                "dashboardP95Ms": p95,
                "dashboardMaxMs": max(ordered) if ordered else None,
                "alerts": {
                    "routes": list(ALERT_ROUTES),
                    "intervalSecs": ALERT_INTERVAL_SECS,
                    "attempts": dict(sorted(self.alert_attempts.items())),
                    "postRestartAttempts": dict(sorted(self.alert_post_restart_attempts.items())),
                    "postRestartSuccesses": dict(sorted(self.alert_post_restart_successes.items())),
                    "postRestartFailures": dict(sorted(self.alert_post_restart_failures.items())),
                    "firstSuccessSecs": dict(sorted(self.alert_first_success_secs.items())),
                    "nonFresh200": dict(sorted(self.alert_non_fresh_200.items())),
                    "postWarm5xx": dict(sorted(self.alert_post_warm_5xx.items())),
                    "postWarmFailures": dict(sorted(self.alert_post_warm_failures.items())),
                    "restartStarted": self.restart_started_at is not None,
                    "restartObserved": self.restart_observed_at is not None,
                },
                "statuses": dict(sorted(self.statuses.items())),
                "errors": dict(sorted(self.errors.items())),
                "events": dict(sorted(self.events.items())),
            }


def alerts_payload_is_fresh(status: int, payload: bytes) -> bool:
    if status != 200:
        return False
    try:
        body = json.loads(payload)
    except (UnicodeDecodeError, json.JSONDecodeError):
        return False
    if not isinstance(body, dict):
        return False
    # Fresh Alerts responses omit coverage; stale last-good responses explicitly
    # carry coverage=stale. Accept an explicit future fresh marker as well.
    return body.get("coverage") in (None, "fresh")


def request(
    recorder: Recorder,
    lane: str,
    method: str,
    host: str,
    port: int,
    path: str,
    body: bytes | None = None,
    headers: dict[str, str] | None = None,
) -> None:
    started = time.monotonic()
    recorder.attempt(lane)
    connection = http.client.HTTPConnection(host, port, timeout=10)
    try:
        connection.request(method, path, body=body, headers=headers or {})
        response = connection.getresponse()
        payload = response.read()
        alert_fresh = (
            alerts_payload_is_fresh(response.status, payload)
            if lane.startswith("alerts_")
            else None
        )
        recorder.status(
            lane,
            response.status,
            (time.monotonic() - started) * 1000,
            alert_fresh=alert_fresh,
        )
    except (OSError, http.client.HTTPException, TimeoutError) as error:
        recorder.error(lane, error)
    finally:
        connection.close()


def create_test_api_key(host: str, port: int) -> None:
    deadline = time.monotonic() + BOOTSTRAP_DEADLINE_SECS
    while True:
        connection = http.client.HTTPConnection(host, port, timeout=10)
        try:
            connection.request(
                "POST",
                "/api/keys",
                body=json.dumps({"api_key": "tvly-load-key"}).encode(),
                headers={"Content-Type": "application/json"},
            )
            response = connection.getresponse()
            response.read()
            if response.status == 201:
                return
            if response.status not in (500, 503) or time.monotonic() >= deadline:
                raise RuntimeError(f"test API-key bootstrap failed: status={response.status}")
        except (OSError, http.client.HTTPException, TimeoutError) as error:
            if time.monotonic() >= deadline:
                raise RuntimeError("test API-key bootstrap did not become available") from error
        finally:
            connection.close()
        time.sleep(0.5)


def create_test_access_token(host: str, port: int) -> str:
    """Create an isolated-load credential after snapshot startup settles.

    The copied production-shaped database may still have one startup maintenance
    writer when the comparison's listener is ready. Retrying only transient
    server failures here prevents that fixture artifact from being classified as
    business-lane coverage, while keeping all retry writes inside the COW test
    database.
    """
    deadline = time.monotonic() + BOOTSTRAP_DEADLINE_SECS
    while True:
        connection = http.client.HTTPConnection(host, port, timeout=10)
        try:
            connection.request(
                "POST",
                "/api/tokens",
                body=json.dumps({"note": "performance-recovery-load"}).encode(),
                headers={"Content-Type": "application/json"},
            )
            response = connection.getresponse()
            payload = response.read()
            if response.status == 201:
                token = json.loads(payload).get("token")
                if isinstance(token, str) and token:
                    return token
                raise RuntimeError("test token bootstrap returned no token")
            if response.status not in (500, 503) or time.monotonic() >= deadline:
                raise RuntimeError(f"test token bootstrap failed: status={response.status}")
        except (OSError, http.client.HTTPException, TimeoutError) as error:
            if time.monotonic() >= deadline:
                raise RuntimeError("test token bootstrap did not become available") from error
        finally:
            connection.close()
        time.sleep(0.5)


def periodic(
    stop: threading.Event,
    interval_secs: float,
    action: callable,
    initial_delay_secs: float = 0.0,
) -> None:
    if stop.wait(initial_delay_secs):
        return
    next_run = time.monotonic()
    while not stop.is_set():
        action()
        next_run = next_periodic_deadline(next_run, interval_secs, time.monotonic())
        stop.wait(max(0.0, next_run - time.monotonic()))


def next_periodic_deadline(
    previous_deadline: float,
    interval_secs: float,
    now: float,
) -> float:
    """Advance a lane without replaying intervals missed by a slow request."""
    scheduled = previous_deadline + interval_secs
    return scheduled if scheduled > now else now + interval_secs


def recovery_tail_secs_for_duration(duration_secs: int, requested_secs: int | None) -> int:
    recovery_tail_secs = requested_secs
    if recovery_tail_secs is None:
        recovery_tail_secs = 60 if duration_secs > 120 else 0
    if recovery_tail_secs < 0 or recovery_tail_secs >= duration_secs:
        raise ValueError("recovery tail must be non-negative and shorter than the total duration")
    return recovery_tail_secs


def dashboard_lane(
    stop: threading.Event,
    recorder: Recorder,
    host: str,
    port: int,
    client_index: int,
) -> None:
    periodic(
        stop,
        DASHBOARD_INTERVAL_SECS,
        lambda: request(recorder, "dashboard", "GET", host, port, "/api/dashboard/overview"),
        client_index * DASHBOARD_INTERVAL_SECS / DASHBOARD_CLIENTS,
    )


def business_lane(
    stop: threading.Event,
    recorder: Recorder,
    host: str,
    port: int,
    access_token: str,
) -> None:
    payload = json.dumps(
        {"query": "snapshot recovery comparison", "search_depth": "basic", "max_results": 1}
    ).encode()
    headers = {
        "Content-Type": "application/json",
        "Authorization": f"Bearer {access_token}",
    }
    periodic(
        stop,
        BUSINESS_INTERVAL_SECS,
        lambda: request(recorder, "business", "POST", host, port, "/api/tavily/search", payload, headers),
    )


def alerts_lane(
    stop: threading.Event,
    recorder: Recorder,
    host: str,
    port: int,
    route: str,
    route_index: int,
) -> None:
    path = f"/api/alerts/{route}"
    if route != "catalog":
        path += "?page=1&per_page=20"
    periodic(
        stop,
        ALERT_INTERVAL_SECS,
        lambda: request(recorder, f"alerts_{route}", "GET", host, port, path),
        route_index * ALERT_INTERVAL_SECS / len(ALERT_ROUTES),
    )


def sse_lane(stop: threading.Event, recorder: Recorder, host: str, port: int) -> None:
    while not stop.is_set():
        connection = http.client.HTTPConnection(host, port, timeout=10)
        try:
            connection.request("GET", "/api/events")
            response = connection.getresponse()
            recorder.status("sse", response.status, 0.0)
            if response.status != 200:
                response.read()
                stop.wait(1.0)
                continue
            while not stop.is_set():
                line = response.fp.readline(4096)
                if not line:
                    break
                if line.startswith(b"event:") or line.startswith(b"data:"):
                    recorder.event("sse_frame")
        except (OSError, http.client.HTTPException, TimeoutError) as error:
            recorder.error("sse", error)
            stop.wait(1.0)
        finally:
            connection.close()


def interrupted_ha_export(stop: threading.Event, recorder: Recorder, host: str, port: int) -> None:
    def interrupt() -> None:
        connection = http.client.HTTPConnection(host, port, timeout=10)
        try:
            connection.request("GET", "/api/admin/ha/events?channel=control&cursor=0")
            response = connection.getresponse()
            recorder.status("ha_export", response.status, 0.0)
            response.read(256)
            recorder.event("ha_export_interrupted")
        except (OSError, http.client.HTTPException, TimeoutError) as error:
            recorder.error("ha_export", error)
        finally:
            connection.close()

    periodic(stop, 30.0, interrupt)


def trigger_ha_gc(stop: threading.Event, recorder: Recorder, host: str, port: int) -> None:
    def trigger() -> None:
        trigger_ha_gc_once(recorder, host, port)

    periodic(stop, 60.0, trigger)


def trigger_ha_gc_once(recorder: Recorder, host: str, port: int) -> None:
    payload = json.dumps({"jobType": "ha_outbox_gc"}).encode()
    headers = {"Content-Type": "application/json"}
    request(recorder, "ha_gc_trigger", "POST", host, port, "/api/jobs/trigger", payload, headers)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--duration-secs", type=int, required=True)
    parser.add_argument("--recovery-tail-secs", type=int)
    parser.add_argument("--restart-marker", type=Path)
    parser.add_argument("--restart-begin-marker", type=Path)
    parser.add_argument("--host", default="app")
    parser.add_argument("--port", type=int, default=8787)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        recovery_tail_secs = recovery_tail_secs_for_duration(
            args.duration_secs,
            args.recovery_tail_secs,
        )
    except ValueError as error:
        parser.error(str(error))
    traffic_duration_secs = args.duration_secs - recovery_tail_secs

    recorder = Recorder(args.restart_marker, args.restart_begin_marker)
    stop = threading.Event()
    create_test_api_key(args.host, args.port)
    access_token = create_test_access_token(args.host, args.port)
    recorder.mark_started()
    threads = [
        threading.Thread(
            target=dashboard_lane,
            args=(stop, recorder, args.host, args.port, client_index),
            daemon=True,
        )
        for client_index in range(DASHBOARD_CLIENTS)
    ]
    threads += [
        threading.Thread(
            target=alerts_lane,
            args=(stop, recorder, args.host, args.port, route, route_index),
            daemon=True,
        )
        for route_index, route in enumerate(ALERT_ROUTES)
    ]
    threads += [
        threading.Thread(target=sse_lane, args=(stop, recorder, args.host, args.port), daemon=True)
        for _ in range(20)
    ]
    threads += [
        *[
            threading.Thread(
                target=business_lane,
                args=(stop, recorder, args.host, args.port, access_token),
                daemon=True,
            )
            for _ in range(BUSINESS_CLIENTS)
        ],
        threading.Thread(target=interrupted_ha_export, args=(stop, recorder, args.host, args.port), daemon=True),
        threading.Thread(target=trigger_ha_gc, args=(stop, recorder, args.host, args.port), daemon=True),
    ]
    started = time.time()
    for thread in threads:
        thread.start()
    try:
        time.sleep(traffic_duration_secs)
    finally:
        stop.set()
        for thread in threads:
            thread.join(timeout=2)
    if recovery_tail_secs:
        # This preserves the ten-minute total while proving that a debt worker
        # can reclaim expired rows once foreground traffic leaves the pool.
        trigger_ha_gc_once(recorder, args.host, args.port)
        time.sleep(recovery_tail_secs)
    summary = recorder.summary()
    summary["durationSecs"] = args.duration_secs
    summary["trafficDurationSecs"] = traffic_duration_secs
    summary["recoveryTailSecs"] = recovery_tail_secs
    summary["dashboardClients"] = DASHBOARD_CLIENTS
    summary["dashboardIntervalSecs"] = DASHBOARD_INTERVAL_SECS
    summary["startedAt"] = int(started)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(summary, sort_keys=True))


if __name__ == "__main__":
    main()
