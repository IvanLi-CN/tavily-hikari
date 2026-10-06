#!/usr/bin/env python3
"""Exercise retention recovery with private synthetic databases and a mock upstream.

Run on a GitHub-hosted runner or the shared testbox, never against a deployed
database. The high-load phase deliberately sustains continuous ten requests/second while
the aged recovery lanes verify and clean the fixed historical target.
"""

import argparse
import concurrent.futures
import http.server
import json
import os
from pathlib import Path
import signal
import socket
import sqlite3
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request


COUNT_FIELDS = "total_requests success_count error_count quota_exhausted_count valuable_success_count valuable_failure_count valuable_failure_429_count other_success_count other_failure_count unknown_count mcp_non_billable mcp_billable api_non_billable api_billable local_estimated_credits".split()


def expected_target_counts(rows=5000):
    counts = dict.fromkeys(COUNT_FIELDS, 0)
    counts.update(
        total_requests=rows,
        success_count=rows,
        valuable_success_count=rows,
        api_billable=rows,
        local_estimated_credits=rows,
    )
    return counts


class MockUpstream(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.respond({"key": {"usage": 0, "limit": 1000000, "search_usage": 0}})

    def do_POST(self):
        self.rfile.read(int(self.headers.get("Content-Length", "0")))
        self.respond({"query": "synthetic", "results": [], "response_time": 0.001, "usage": {"credits": 1}})

    def respond(self, payload):
        body = json.dumps(payload).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


def request(origin, path, payload=None, token=None):
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = "Bearer " + token
    req = urllib.request.Request(origin + path, data=json.dumps(payload).encode() if payload is not None else None, headers=headers)
    start = time.monotonic()
    try:
        with urllib.request.urlopen(req, timeout=10) as response:
            return response.status, time.monotonic() - start, response.read()
    except urllib.error.HTTPError as error:
        return error.code, time.monotonic() - start, error.read()
    except (OSError, TimeoutError) as error:
        return 599, time.monotonic() - start, str(error).encode()


def percentile95(values):
    return sorted(values)[min(len(values) - 1, int(len(values) * 0.95))] * 1000


def stop(process):
    process.send_signal(signal.SIGTERM)
    try:
        process.wait(timeout=30)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)
        raise RuntimeError("synthetic service failed to shut down within 30 seconds")


def rollup_counts(conn, bucket_secs, start, end):
    fields = ", ".join(f"COALESCE(SUM({field}), 0)" for field in COUNT_FIELDS)
    row = conn.execute(
        f"SELECT {fields} FROM dashboard_request_rollup_buckets WHERE bucket_secs=? AND bucket_start >= ? AND bucket_start < ?",
        (bucket_secs, start, end),
    ).fetchone()
    return dict(zip(COUNT_FIELDS, row))


def exact_rollup_counts(conn, bucket_secs, bucket_start):
    fields = ", ".join(COUNT_FIELDS)
    row = conn.execute(
        f"SELECT {fields} FROM dashboard_request_rollup_buckets WHERE bucket_secs=? AND bucket_start=?",
        (bucket_secs, bucket_start),
    ).fetchone()
    return dict(zip(COUNT_FIELDS, row)) if row else None


def fixture_identity(conn, start, end):
    return conn.execute(
        "SELECT COUNT(*), MIN(id), MAX(id), COALESCE(SUM(id), 0) FROM request_logs WHERE created_at >= ? AND created_at < ?",
        (start, end),
    ).fetchone()


def billing_truth(conn):
    return conn.execute(
        "SELECT COUNT(*), COALESCE(SUM(business_credits), 0), SUM(CASE WHEN billing_state='charged' THEN 1 ELSE 0 END) FROM billing_ledger WHERE token_id='synthetic-recovery-billing'"
    ).fetchone()


def recovery_complete(state, expected_counts):
    return (
        state["expired"] == 0
        and state["target_seal"] == expected_counts
        and state["target_daily_rollup"] == expected_counts
        and state["target_minute_rollup"] == expected_counts
    )


def validate_recovery_consistency(state, expected_counts, expected_billing, expected_retained_identity):
    assert state["target_seal"] == expected_counts, state
    assert state["target_daily_rollup"] == expected_counts, state
    assert state["target_minute_rollup"] == expected_counts, state
    assert state["billing_truth"] == expected_billing, state
    assert state["retained_fixture_identity"] == expected_retained_identity, state
    assert state["retained_fixture_rows"] == expected_retained_identity[0], state


def progress_marker(state):
    return json.dumps(
        {
            "expired": state["expired"],
            "pending_days": state["pending_days"],
            "blocking_work": state["blocking_work"],
            "integrity_work": state["integrity_work"],
            "target_seal": state["target_seal"],
            "target_minute_rollup": state["target_minute_rollup"],
            "target_daily_rollup": state["target_daily_rollup"],
        },
        sort_keys=True,
    )


def progress_block_reason(state):
    if state["active_gc"] or any(row[1] in ("queued", "running") for row in state["gc_jobs"]):
        return "gc_jobs_not_advancing"
    if state["blocking_work"] or state["pending_days"] or any(row[1] in ("queued", "running") for row in state["integrity_jobs"]):
        return "integrity_jobs_not_advancing"
    return "no_runnable_recovery_lane"


class EffectiveProgressTracker:
    def __init__(self, state):
        self.marker = progress_marker(state)
        self.last_progress_elapsed = 0
        self.effective_progress_checks = 0

    def observe(self, elapsed, state, recovered):
        marker = progress_marker(state)
        if marker != self.marker:
            self.marker = marker
            self.last_progress_elapsed = elapsed
            self.effective_progress_checks += 1
        elif not recovered and elapsed - self.last_progress_elapsed >= 300:
            raise AssertionError(
                {
                    "reason": progress_block_reason(state),
                    "elapsed": elapsed,
                    "last_progress_elapsed": self.last_progress_elapsed,
                    "state": state,
                }
            )


def snapshot(core, sidecar, threshold, retained_start, retained_end):
    target_day = retained_start - 4 * 86400
    with sqlite3.connect(sidecar, timeout=1) as conn:
        old = conn.execute("SELECT COUNT(*) FROM request_logs WHERE created_at < ?", (threshold,)).fetchone()[0]
        retained = conn.execute("SELECT COUNT(*) FROM request_logs WHERE created_at >= ? AND created_at < ?", (retained_start, retained_end)).fetchone()[0]
        pending = conn.execute("SELECT bucket_start,cursor,gc_blocking FROM dashboard_rollup_integrity_day_reaudits ORDER BY bucket_start").fetchall()
        hot = conn.execute("SELECT hot_cursor,hot_fence FROM dashboard_rollup_integrity_state WHERE id=1").fetchone()
        seal_row = conn.execute("SELECT counts_json FROM dashboard_rollup_daily_seals WHERE bucket_start=?", (target_day,)).fetchone()
        seal = json.loads(seal_row[0]) if seal_row else None
        daily = exact_rollup_counts(conn, 86400, target_day)
        minute = rollup_counts(conn, 60, target_day, target_day + 86400)
        integrity_work = conn.execute("SELECT range_start,range_end,cursor_created_at,cursor_id,priority,status FROM dashboard_rollup_integrity_work_items WHERE status='pending' ORDER BY priority DESC,range_start LIMIT 16").fetchall()
        blocking_work = conn.execute("SELECT w.range_start,w.range_end,w.cursor_created_at,w.cursor_id,w.priority,w.status FROM dashboard_rollup_integrity_day_reaudits d JOIN dashboard_rollup_integrity_work_items w ON w.range_start >= d.bucket_start AND w.range_end <= d.bucket_end WHERE d.gc_blocking=1 AND d.status='pending' AND w.status='pending' ORDER BY d.updated_at,w.range_start LIMIT 8").fetchall()
        work_priority_counts = conn.execute("SELECT priority,COUNT(*) FROM dashboard_rollup_integrity_work_items WHERE status='pending' GROUP BY priority ORDER BY priority DESC").fetchall()
        retained_identity = fixture_identity(conn, retained_start, retained_end)
        target_source_rows = conn.execute("SELECT COUNT(*) FROM request_logs WHERE created_at >= ? AND created_at < ?", (target_day, target_day + 86400)).fetchone()[0]
    with sqlite3.connect(core, timeout=1) as conn:
        active = conn.execute("SELECT COUNT(*) FROM scheduled_jobs WHERE job_type='request_logs_gc' AND status IN ('queued','running')").fetchone()[0]
        messages = conn.execute("SELECT id,status,message FROM scheduled_jobs WHERE job_type='request_logs_gc' ORDER BY id DESC LIMIT 3").fetchall()
        integrity_jobs = conn.execute("SELECT id,status,message FROM scheduled_jobs WHERE job_type='dashboard_rollup_integrity' ORDER BY id DESC LIMIT 3").fetchall()
        truth = billing_truth(conn)
    return {"expired": old, "retained_fixture_rows": retained, "retained_fixture_identity": retained_identity, "target_source_rows": target_source_rows, "pending_days": pending, "hot": hot, "target_seal": seal, "target_minute_rollup": minute, "target_daily_rollup": daily, "integrity_work": integrity_work, "blocking_work": blocking_work, "work_priority_counts": work_priority_counts, "active_gc": active, "gc_jobs": messages, "integrity_jobs": integrity_jobs, "billing_truth": truth}


def load(origin, token, seconds, rps, on_tick=None, stop_when=None):
    results = []
    futures = []
    start = time.monotonic()
    next_tick = start
    with concurrent.futures.ThreadPoolExecutor(max_workers=32) as executor:
        for index in range(int(seconds * rps)):
            target = start + index / rps
            time.sleep(max(0, target - time.monotonic()))
            futures.append(executor.submit(request, origin, "/api/tavily/search", {"query": "synthetic recovery load", "max_results": 1}, token))
            now = time.monotonic()
            if on_tick and now >= next_tick:
                failed = [future.result()[0] for future in futures if future.done() and future.result()[0] != 200]
                assert not failed, failed[:3]
                on_tick(round(now - start))
                next_tick = now + 60
            if stop_when and stop_when():
                break
        results = [future.result() for future in futures]
    failures = [(status, body[:200].decode(errors="replace")) for status, _, body in results if status != 200]
    return {"requests": len(results), "rps": rps, "duration_secs": round(time.monotonic() - start, 2), "p95_ms": percentile95([elapsed for _, elapsed, _ in results]), "non_200": len(failures), "failure_samples": failures[:3]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--agent-dir", type=Path, required=True)
    parser.add_argument("--candidate-sha", required=True)
    parser.add_argument("--high-seconds", type=int, default=1800)
    parser.add_argument("--low-seconds", type=int, default=1800)
    parser.add_argument("--low-rps", type=float, default=0.1)
    parser.add_argument("--baseline-seconds", type=int, default=60)
    args = parser.parse_args()
    root = args.agent_dir.resolve()
    allowed_roots = [Path("/srv/codex/agents")]
    runner_temp = os.environ.get("RUNNER_TEMP")
    if (
        os.environ.get("GITHUB_ACTIONS") == "true"
        and os.environ.get("RUNNER_ENVIRONMENT") == "github-hosted"
        and runner_temp
    ):
        allowed_roots.append(Path(runner_temp).resolve())
    if not any(root.is_relative_to(parent) and root != parent for parent in allowed_roots):
        parser.error(
            "--agent-dir must be below /srv/codex/agents or a GitHub-hosted RUNNER_TEMP"
        )
    if min(args.high_seconds, args.low_seconds, args.baseline_seconds) <= 0:
        parser.error("phase durations must be positive")
    if not 0 < args.low_rps <= 5:
        parser.error("--low-rps must be positive and within the accepted five-request/second limit")
    run_dir = Path(tempfile.mkdtemp(prefix="gc-recovery-load-", dir=root))
    core = run_dir / "fixture.db"
    sidecar = run_dir / "fixture-observability.db"
    mock = http.server.ThreadingHTTPServer(("127.0.0.1", 0), MockUpstream)
    threading.Thread(target=mock.serve_forever, daemon=True).start()
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    origin = f"http://127.0.0.1:{port}"
    mock_origin = f"http://127.0.0.1:{mock.server_port}"
    env = dict(os.environ, PROXY_DB_PATH=str(core), PROXY_BIND="127.0.0.1", PROXY_PORT=str(port), TAVILY_API_KEYS="tvly-synthetic-test-key", TAVILY_UPSTREAM=mock_origin, TAVILY_USAGE_BASE=mock_origin, API_KEY_IP_GEO_ORIGIN=mock_origin, DEV_OPEN_ADMIN="true", HA_MODE="single", REQUEST_LOGS_RETENTION_DAYS="7", TOKEN_HOURLY_LIMIT="1000000", TOKEN_DAILY_LIMIT="1000000", TOKEN_MONTHLY_LIMIT="1000000", TOKEN_HOURLY_REQUEST_LIMIT="1000000", TZ="Asia/Shanghai")
    processes = []
    logs = []

    def start_service(background):
        log = (run_dir / ("service-enabled.log" if background else f"service-disabled-{len(logs)}.log")).open("wb")
        logs.append(log)
        process = subprocess.Popen([str(args.binary.resolve())], cwd=run_dir, env=dict(env, TAVILY_DISABLE_BACKGROUND_TASKS="false" if background else "true"), stdout=log, stderr=subprocess.STDOUT)
        processes.append(process)
        for _ in range(60):
            if process.poll() is not None:
                raise RuntimeError(f"synthetic service exited {process.returncode}; inspect {log.name}")
            if request(origin, "/health")[0] == 200:
                return process
            time.sleep(0.5)
        raise RuntimeError("synthetic service readiness timed out")

    try:
        process = start_service(False)
        status, _, body = request(origin, "/api/tokens", {"note": "private GC recovery probe"})
        assert status == 201, (status, body)
        token = json.loads(body)["token"]
        stop(process)
        with sqlite3.connect(core) as conn:
            conn.executemany("INSERT OR REPLACE INTO meta(key,value) VALUES (?,?)", [("request_log_retention_max_days_v1", "7"), ("request_rate_limit_v1", "50000")])
        now = int(time.time())
        closed = now - now % 300
        day = (now + 28800) // 86400 * 86400 - 28800 - 10 * 86400
        retained_day = day + 4 * 86400
        threshold = (now + 28800) // 86400 * 86400 - 28800 - 7 * 86400
        with sqlite3.connect(sidecar) as conn:
            conn.executemany("INSERT INTO request_logs (method,path,result_status,request_kind_key,status_code,tavily_status_code,visibility,created_at,business_credits,counts_business_quota,request_body) VALUES ('POST','/api/tavily/search','success','api:search',200,200,'visible',?,1,1,?)", ((day + index * 86399 // 5000, b'{"query":"synthetic expired source"}') for index in range(5000)))
            conn.executemany("INSERT INTO request_logs (method,path,result_status,request_kind_key,status_code,tavily_status_code,visibility,created_at,business_credits,counts_business_quota,request_body) VALUES ('POST','/api/tavily/search','success','api:search',200,200,'visible',?,1,1,?)", ((retained_day + index * 86399 // 95000, b'{"query":"synthetic retained source"}') for index in range(95000)))
            conn.execute("INSERT OR REPLACE INTO dashboard_rollup_integrity_state (id,hot_cursor,hot_fence,history_cursor,hot_reaudit_cursor,last_history_attempt_at,last_seal_attempt_at,updated_at) VALUES (1,?,?,?,?,?,?,?)", (closed, closed, closed - 86400, closed - 86400, now, now, now))
            bad = dict.fromkeys(COUNT_FIELDS, 0)
            bad.update(total_requests=5000, success_count=5000, valuable_success_count=5000, api_billable=5000, local_estimated_credits=4999)
            conn.execute("INSERT INTO dashboard_rollup_daily_seals VALUES (?,?,?)", (day, json.dumps(bad), now))
        with sqlite3.connect(core) as conn:
            conn.executemany(
                "INSERT OR REPLACE INTO billing_ledger (auth_token_log_id,token_id,billing_state,business_credits,result_status,created_at,updated_at) VALUES (?, 'synthetic-recovery-billing', 'charged', 1, 'success', ?, ?)",
                ((1_000_000 + index, day, now) for index in range(5000)),
            )
        process = start_service(False)
        status, _, body = request(origin, "/api/tavily/search", {"query": "synthetic preflight", "max_results": 1}, token)
        assert status == 200, (status, body)
        baseline = load(origin, token, args.baseline_seconds, 10)
        assert baseline["non_200"] == 0, baseline
        stop(process)
        process = start_service(True)
        triggered = False
        recovered = False
        recovery_elapsed = None
        expected_counts = expected_target_counts()
        high_start = snapshot(core, sidecar, threshold, retained_day, retained_day + 86400)
        initial_expired = high_start["expired"]
        assert initial_expired == 5000, high_start
        assert high_start["retained_fixture_rows"] == 95000, high_start
        expected_billing = high_start["billing_truth"]
        expected_retained_identity = high_start["retained_fixture_identity"]
        progress_tracker = None

        def high_tick(elapsed):
            nonlocal triggered, recovered, recovery_elapsed, progress_tracker
            if elapsed >= 60 and not triggered:
                status, _, body = request(origin, "/api/jobs/trigger", {"jobType": "request_logs_gc"})
                assert status in (200, 202), (status, body)
                triggered = True
            state = snapshot(core, sidecar, threshold, retained_day, retained_day + 86400)
            if triggered and progress_tracker is None:
                progress_tracker = EffectiveProgressTracker(state)
            recovered = recovery_complete(state, expected_counts)
            if recovered and recovery_elapsed is None:
                recovery_elapsed = elapsed
            if progress_tracker is not None:
                progress_tracker.observe(elapsed, state, recovered)
            print(json.dumps({"phase": "high", "elapsed": elapsed, **state}), flush=True)

        high = load(origin, token, args.high_seconds, 10, high_tick, lambda: recovered)
        high_end = snapshot(core, sidecar, threshold, retained_day, retained_day + 86400)
        assert triggered, "high phase must be long enough to trigger GC after foreground pressure stabilizes"
        assert baseline["non_200"] == high["non_200"] == 0, (baseline, high)
        assert high["p95_ms"] - baseline["p95_ms"] <= 250, (baseline, high)
        assert recovered, "fixed-target recovery must complete during continuous 10 RPS traffic"
        assert recovery_elapsed is not None and recovery_elapsed <= args.high_seconds, high_end
        assert initial_expired - high_end["expired"] >= 5000, high_end
        assert high_end["retained_fixture_rows"] == 95000, high_end
        validate_recovery_consistency(
            high_end,
            expected_counts,
            expected_billing,
            expected_retained_identity,
        )
        with sqlite3.connect(core) as conn:
            defers = conn.execute("SELECT COUNT(*) FROM scheduled_jobs WHERE job_type='request_logs_gc' AND message LIKE '%foreground_pressure%' AND status='success'").fetchone()[0]
        assert defers > 0, "foreground pressure must cause a durable GC defer"
        high_evidence = {"candidate_sha": args.candidate_sha, "fixture_rows": 100000, "expired_fixture_rows": initial_expired, "retained_fixture_rows": high_start["retained_fixture_rows"], "baseline": baseline, "high": high, "foreground_defers": defers, "recovery_elapsed_secs": recovery_elapsed, "high_target_recovered": recovered, "effective_progress_checks": progress_tracker.effective_progress_checks if progress_tracker else 0, "last_progress_elapsed": progress_tracker.last_progress_elapsed if progress_tracker else None}
        (run_dir / "high-evidence.json").write_text(json.dumps(high_evidence, indent=2) + "\n")
        print(json.dumps({"phase": "high-complete", **high_evidence}), flush=True)

        def low_tick(elapsed):
            nonlocal recovered
            state = snapshot(core, sidecar, threshold, retained_day, retained_day + 86400)
            assert state["active_gc"] <= 1, state
            recovered = recovered or recovery_complete(state, expected_counts)
            (run_dir / "latest-progress.json").write_text(json.dumps({"phase": "low", "elapsed": elapsed, **state}, indent=2) + "\n")
            print(json.dumps({"phase": "low", "elapsed": elapsed, **state}), flush=True)

        low = load(origin, token, args.low_seconds, args.low_rps, low_tick, lambda: recovered)
        final = snapshot(core, sidecar, threshold, retained_day, retained_day + 86400)
        if low["non_200"] != 0 or not recovered or initial_expired - final["expired"] < 5000 or final["retained_fixture_rows"] != 95000:
            failure = {"candidate_sha": args.candidate_sha, "fixture_rows": 100000, "expired_fixture_rows": initial_expired, "low": low, "final": final, "passed": False}
            (run_dir / "recovery-failure-evidence.json").write_text(json.dumps(failure, indent=2) + "\n")
            raise AssertionError((low, final))
        validate_recovery_consistency(
            final,
            expected_counts,
            expected_billing,
            expected_retained_identity,
        )
        evidence = {"candidate_sha": args.candidate_sha, "fixture_rows": 100000, "expired_fixture_rows": initial_expired, "retained_fixture_rows": high_start["retained_fixture_rows"], "baseline": baseline, "high": high, "foreground_defers": defers, "low": low, "deleted_expired": initial_expired - final["expired"], "final": final, "recovered_seal": final["target_seal"], "expected_target_counts": expected_counts, "billing_truth": final["billing_truth"], "effective_progress_checks": progress_tracker.effective_progress_checks if progress_tracker else 0, "last_progress_elapsed": progress_tracker.last_progress_elapsed if progress_tracker else None, "passed": True}
        (run_dir / "evidence.json").write_text(json.dumps(evidence, indent=2) + "\n")
        print(json.dumps({"evidence": str(run_dir / "evidence.json"), **evidence}), flush=True)
    finally:
        for process in processes:
            if process.poll() is None:
                stop(process)
        mock.shutdown()
        for log in logs:
            log.close()


if __name__ == "__main__":
    main()
