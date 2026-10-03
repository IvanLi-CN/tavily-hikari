#!/usr/bin/env python3
"""Exercise retention recovery with private synthetic databases and a mock upstream.

Run on the shared testbox, never against a deployed database. The high-load phase
keeps the existing five-request/second admission threshold; the low-load phase
then measures source-backed seal recovery and bounded GC catch-up.
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


def snapshot(core, sidecar, threshold):
    with sqlite3.connect(sidecar, timeout=1) as conn:
        old = conn.execute("SELECT COUNT(*) FROM request_logs WHERE created_at < ?", (threshold,)).fetchone()[0]
        pending = conn.execute("SELECT bucket_start,cursor,gc_blocking FROM dashboard_rollup_integrity_day_reaudits ORDER BY bucket_start").fetchall()
        hot = conn.execute("SELECT hot_cursor,hot_fence FROM dashboard_rollup_integrity_state WHERE id=1").fetchone()
    with sqlite3.connect(core, timeout=1) as conn:
        active = conn.execute("SELECT COUNT(*) FROM scheduled_jobs WHERE job_type='request_logs_gc' AND status IN ('queued','running')").fetchone()[0]
        messages = conn.execute("SELECT id,status,message FROM scheduled_jobs WHERE job_type='request_logs_gc' ORDER BY id DESC LIMIT 3").fetchall()
    return {"expired": old, "pending_days": pending, "hot": hot, "active_gc": active, "gc_jobs": messages}


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
    parser.add_argument("--low-seconds", type=int, default=1200)
    parser.add_argument("--baseline-seconds", type=int, default=60)
    args = parser.parse_args()
    root = args.agent_dir.resolve()
    if not root.is_relative_to(Path("/srv/codex/agents")) or root == Path("/srv/codex/agents"):
        parser.error("--agent-dir must be a task-owned directory below /srv/codex/agents")
    if min(args.high_seconds, args.low_seconds, args.baseline_seconds) <= 0:
        parser.error("phase durations must be positive")
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
        threshold = (now + 28800) // 86400 * 86400 - 28800 - 7 * 86400
        with sqlite3.connect(sidecar) as conn:
            conn.executemany("INSERT INTO request_logs (method,path,result_status,request_kind_key,status_code,tavily_status_code,visibility,created_at,business_credits,counts_business_quota,request_body) VALUES ('POST','/api/tavily/search','success','api:search',200,200,'visible',?,1,1,?)", ((day + (index // 50000) * 86400 + (index % 50000) * 86399 // 50000, b'{"query":"synthetic old source"}') for index in range(100000)))
            conn.execute("INSERT OR REPLACE INTO dashboard_rollup_integrity_state (id,hot_cursor,hot_fence,history_cursor,hot_reaudit_cursor,last_history_attempt_at,last_seal_attempt_at,updated_at) VALUES (1,?,?,?,?,?,?,?)", (closed, closed, closed - 86400, closed - 86400, now, now, now))
            bad = dict.fromkeys(COUNT_FIELDS, 0)
            bad.update(total_requests=50000, success_count=50000, valuable_success_count=50000, api_billable=50000, local_estimated_credits=49999)
            conn.execute("INSERT INTO dashboard_rollup_daily_seals VALUES (?,?,?)", (day, json.dumps(bad), now))
        process = start_service(False)
        baseline = load(origin, token, args.baseline_seconds, 10)
        stop(process)
        process = start_service(True)
        triggered = False
        high_start = snapshot(core, sidecar, threshold)

        def high_tick(elapsed):
            nonlocal triggered
            if elapsed >= 60 and not triggered:
                status, _, body = request(origin, "/api/jobs/trigger", {"jobType": "request_logs_gc"})
                assert status in (200, 202), (status, body)
                triggered = True
            print(json.dumps({"phase": "high", "elapsed": elapsed, **snapshot(core, sidecar, threshold)}), flush=True)

        high = load(origin, token, args.high_seconds, 10, high_tick)
        high_end = snapshot(core, sidecar, threshold)
        assert triggered, "high phase must be long enough to trigger GC after foreground pressure stabilizes"
        assert baseline["non_200"] == high["non_200"] == 0, (baseline, high)
        assert high["p95_ms"] - baseline["p95_ms"] <= 250, (baseline, high)
        assert high_end["expired"] == high_start["expired"] == 100000, high_end
        with sqlite3.connect(core) as conn:
            defers = conn.execute("SELECT COUNT(*) FROM scheduled_jobs WHERE job_type='request_logs_gc' AND message LIKE '%foreground_pressure%' AND status='success'").fetchone()[0]
        assert defers > 0, "foreground pressure must cause a durable GC defer"
        recovered = False

        def low_tick(elapsed):
            nonlocal recovered
            state = snapshot(core, sidecar, threshold)
            assert state["active_gc"] <= 1, state
            recovered = 100000 - state["expired"] >= 5000
            print(json.dumps({"phase": "low", "elapsed": elapsed, **state}), flush=True)

        low = load(origin, token, args.low_seconds, 1, low_tick, lambda: recovered)
        final = snapshot(core, sidecar, threshold)
        assert low["non_200"] == 0 and 100000 - final["expired"] >= 5000, (low, final)
        with sqlite3.connect(sidecar) as conn:
            seal = json.loads(conn.execute("SELECT counts_json FROM dashboard_rollup_daily_seals WHERE bucket_start=?", (day,)).fetchone()[0])
            daily = conn.execute("SELECT total_requests,local_estimated_credits FROM dashboard_request_rollup_buckets WHERE bucket_start=? AND bucket_secs=86400", (day,)).fetchone()
            assert seal["total_requests"] == seal["local_estimated_credits"] == 50000, seal
            assert daily == (50000, 50000), daily
        evidence = {"candidate_sha": args.candidate_sha, "fixture_rows": 100000, "baseline": baseline, "high": high, "foreground_defers": defers, "low": low, "deleted_expired": 100000 - final["expired"], "final": final, "recovered_seal": seal, "passed": True}
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
