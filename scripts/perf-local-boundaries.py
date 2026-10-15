#!/usr/bin/env python3
"""Run paired local request-cost measurements against a mock-only upstream.

The harness intentionally does not build, deploy, or contact a production service. A caller
supplies a service command template for each archived source revision. The command receives the
temporary fixture paths and fixed workload settings through PERF_LOCAL_* environment variables.
"""

from __future__ import annotations

import argparse
import http.client
import json
import os
import re
import shlex
import signal
import socket
import sqlite3
import statistics
import subprocess
import tarfile
import tempfile
import threading
import time
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any, Callable


DEFAULT_BASELINE_REF = "b1bb1043823b9260b18389854e6b90b2cdedbd70"
DEFAULT_WARMUP_SECONDS = 60
DEFAULT_DURATION_SECONDS = 600
DEFAULT_ROUNDS = 3
DEFAULT_EXTENSION_SECONDS = 1800
DEFAULT_MOCK_DELAY_MS = 200
DEFAULT_MIN_SAMPLES = 30
DEFAULT_REQUEST_INTERVAL_SECONDS = 0.25
DEFAULT_SUBJECTS = ("subject-a", "subject-b", "subject-c", "subject-d")
DEFAULT_TOKENS = ("token-a", "token-b", "token-c", "token-d")
DEFAULT_RESOURCES = "cpu=2,memory=1GiB,sqlite_pool=4"
MAX_SERVICE_LOG_BYTES = 64 * 1024
PHASE_METRIC_SELECTOR = {
    "method": "POST",
    "operation": "search",
    "outcome": "success",
    "route_family": "http_search",
}


class MockUpstreamHandler(BaseHTTPRequestHandler):
    delay_seconds = DEFAULT_MOCK_DELAY_MS / 1000

    def do_GET(self) -> None:  # noqa: N802
        self._respond()

    def do_POST(self) -> None:  # noqa: N802
        length = int(self.headers.get("Content-Length", "0"))
        if length:
            self.rfile.read(min(length, 64 * 1024))
        self._respond()

    def log_message(self, _format: str, *_args: object) -> None:
        return

    def _respond(self) -> None:
        time.sleep(self.delay_seconds)
        payload = {
            "results": [{"title": "synthetic", "url": "http://sandbox.invalid/result"}],
            "usage": {"credits": 1},
        }
        encoded = json.dumps(payload, separators=(",", ":")).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)


class MockUpstream:
    def __init__(self, delay_ms: int) -> None:
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), MockUpstreamHandler)
        MockUpstreamHandler.delay_seconds = delay_ms / 1000
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    @property
    def url(self) -> str:
        return f"http://127.0.0.1:{self.server.server_port}"

    def __enter__(self) -> "MockUpstream":
        self.thread.start()
        return self

    def __exit__(self, *_exc: object) -> None:
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)


class HarnessInterrupted(RuntimeError):
    """Raised by the harness signal handler so checkpoints can be finalized."""


def atomic_write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary_path: str | None = None
    try:
        descriptor, temporary_path = tempfile.mkstemp(
            prefix=f".{path.name}.",
            dir=path.parent,
        )
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            json.dump(payload, handle, indent=2, sort_keys=True)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary_path, path)
        temporary_path = None
    finally:
        if temporary_path is not None:
            try:
                os.unlink(temporary_path)
            except FileNotFoundError:
                pass


def redact_output(text: str) -> str:
    redacted = re.sub(r"(?i)(authorization\s*[:=]\s*bearer\s+)[^\s,]+", r"\1<REDACTED>", text)
    redacted = re.sub(r"(?i)(bearer\s+)[^\s,]+", r"\1<REDACTED>", redacted)
    redacted = re.sub(r"\btvly-[A-Za-z0-9._~-]+\b", "<REDACTED>", redacted)
    return re.sub(
        r"(?i)((?:password|secret|token)\s*[:=]\s*)[^\s,]+",
        r"\1<REDACTED>",
        redacted,
    )


class BoundedLogWriter:
    def __init__(self, path: Path, max_bytes: int = MAX_SERVICE_LOG_BYTES) -> None:
        self.path = path
        self.max_bytes = max_bytes
        self._written = 0
        self._truncated = False
        self._handle = path.open("wb")

    def write(self, chunk: bytes) -> None:
        if self._truncated:
            return
        encoded = redact_output(chunk.decode("utf-8", errors="replace")).encode("utf-8")
        remaining = self.max_bytes - self._written
        if remaining <= 0:
            self._truncated = True
            return
        if len(encoded) > remaining:
            encoded = encoded[:remaining]
            self._truncated = True
        self._handle.write(encoded)
        self._written += len(encoded)

    def close(self) -> None:
        if self._truncated and self._written < self.max_bytes:
            marker = b"\n<LOG_TRUNCATED>\n"
            self._handle.write(marker[: self.max_bytes - self._written])
        self._handle.flush()
        os.fsync(self._handle.fileno())
        self._handle.close()


def drain_process_output(stream: Any, sink: BoundedLogWriter) -> None:
    try:
        while True:
            chunk = stream.read(4096)
            if not chunk:
                return
            sink.write(chunk)
    finally:
        stream.close()


class CheckpointStore:
    """Persist bounded, secret-free run evidence independently of final output."""

    def __init__(self, root: Path, contract: dict[str, Any]) -> None:
        self.root = root.resolve()
        self.variant_root = self.root / "variants"
        self.pair_root = self.root / "pairs"
        self.log_root = self.root / "logs"
        self.variant_root.mkdir(parents=True, exist_ok=True)
        self.pair_root.mkdir(parents=True, exist_ok=True)
        self.log_root.mkdir(parents=True, exist_ok=True)
        self.active: dict[str, Any] | None = None
        atomic_write_json(self.root / "contract.json", {"kind": "contract", "contract": contract})
        self._write_progress("started")

    def variant_path(self, phase: str, round_number: int, variant: str) -> Path:
        return self.variant_root / f"{phase}-round-{round_number}-{variant}.json"

    def log_directory(self, phase: str, round_number: int, variant: str) -> Path:
        directory = self.log_root / phase / f"round-{round_number}-{variant}"
        directory.mkdir(parents=True, exist_ok=True)
        return directory

    def _write_progress(self, status: str, **extra: Any) -> None:
        payload: dict[str, Any] = {
            "kind": "progress",
            "status": status,
            "updated_at_unix": time.time(),
        }
        if self.active is not None:
            payload["active"] = self.active
        payload.update(extra)
        atomic_write_json(self.root / "progress.json", payload)

    def write_variant_started(
        self,
        phase: str,
        round_number: int,
        variant: str,
        ref: str,
        duration_seconds: int,
    ) -> None:
        self.active = {
            "phase": phase,
            "round": round_number,
            "variant": variant,
            "ref": ref,
            "duration_seconds": duration_seconds,
        }
        atomic_write_json(
            self.variant_path(phase, round_number, variant),
            {
                "kind": "variant_checkpoint",
                **self.active,
                "status": "running",
                "reason": "variant_started",
                "request": None,
                "unrelated_cohort": None,
                "statuses": {},
                "errors": {},
                "phases": {},
            },
        )
        self._write_progress("variant_running")

    def write_variant_result(
        self,
        phase: str,
        round_number: int,
        variant: str,
        result: dict[str, Any],
    ) -> None:
        payload = {
            "kind": "variant_checkpoint",
            "phase": phase,
            "round": round_number,
            "variant": variant,
            "ref": result.get("ref"),
            "status": result.get("status"),
            "reason": result.get("reason"),
            "duration_seconds": result.get("duration_seconds"),
            "request": result.get("request"),
            "unrelated_cohort": result.get("unrelated_cohort"),
            "statuses": result.get("statuses", {}),
            "errors": result.get("errors", {}),
            "phases": result.get("phases", {}),
            "cohort": result.get("cohort"),
            "accepted_progress": result.get("accepted_progress"),
        }
        if result.get("error_type") is not None:
            payload["error_type"] = result["error_type"]
        atomic_write_json(self.variant_path(phase, round_number, variant), payload)
        self.active = None
        self._write_progress("variant_complete", phase=phase, round=round_number, variant=variant)

    def write_pair(
        self,
        phase: str,
        round_number: int,
        baseline: dict[str, Any],
        candidate: dict[str, Any],
        comparison: dict[str, Any],
    ) -> None:
        atomic_write_json(
            self.pair_root / f"{phase}-round-{round_number}.json",
            {
                "kind": "pair_checkpoint",
                "phase": phase,
                "round": round_number,
                "status": comparison.get("status"),
                "reasons": comparison.get("reasons", []),
                "baseline": baseline,
                "candidate": candidate,
                "comparison": comparison,
            },
        )
        self._write_progress("pair_complete", phase=phase, round=round_number)

    def mark_interrupted(self, reason: str) -> None:
        if self.active is not None:
            interrupted = {
                "kind": "variant_checkpoint",
                **self.active,
                "status": "interrupted",
                "reason": reason,
                "request": None,
                "unrelated_cohort": None,
                "statuses": {},
                "errors": {},
                "phases": {},
            }
            atomic_write_json(
                self.variant_path(
                    self.active["phase"], self.active["round"], self.active["variant"]
                ),
                interrupted,
            )
        self.active = None
        self._write_progress("interrupted", reason=reason)

    def mark_finished(self, status: str) -> None:
        self.active = None
        self._write_progress("finished", result_status=status)


@dataclass
class SampleRecorder:
    started_at: float = field(default_factory=time.monotonic)
    elapsed_ms: list[float] = field(default_factory=list)
    unrelated_elapsed_ms: list[float] = field(default_factory=list)
    statuses: dict[str, int] = field(default_factory=dict)
    errors: dict[str, int] = field(default_factory=dict)
    accepted_progress: int = 0
    reported_progress: int = 0

    def add_status(self, lane: str, status: int, elapsed_ms: float, body: bytes) -> None:
        key = f"{lane}:{status}"
        self.statuses[key] = self.statuses.get(key, 0) + 1
        target = self.unrelated_elapsed_ms if lane == "unrelated" else self.elapsed_ms
        target.append(elapsed_ms)
        if lane != "unrelated" and 200 <= status < 300:
            self.accepted_progress += 1
        try:
            decoded = json.loads(body)
        except (ValueError, UnicodeDecodeError):
            return
        if isinstance(decoded, dict):
            value = decoded.get("accepted_progress")
            if isinstance(value, int) and value >= 0:
                self.reported_progress += value

    def add_error(self, lane: str, error: BaseException) -> None:
        key = f"{lane}:{type(error).__name__}"
        self.errors[key] = self.errors.get(key, 0) + 1

    def stats(self, values: list[float], duration_seconds: int) -> dict[str, Any]:
        ordered = sorted(values)
        p95 = ordered[min(len(ordered) - 1, int(len(ordered) * 0.95))] if ordered else None
        return {
            "samples": len(values),
            "mean_ms": round(statistics.fmean(values), 3) if values else None,
            "p95_ms": round(p95, 3) if p95 is not None else None,
            "throughput_rps": round(len(values) / max(duration_seconds, 1), 4),
        }

    def result(self, duration_seconds: int) -> dict[str, Any]:
        return {
            "duration_seconds": duration_seconds,
            "request": self.stats(self.elapsed_ms, duration_seconds),
            "unrelated_cohort": self.stats(self.unrelated_elapsed_ms, duration_seconds),
            "statuses": dict(sorted(self.statuses.items())),
            "errors": dict(sorted(self.errors.items())),
            "phases": {},
            "accepted_progress": {
                "responses": self.accepted_progress,
                "reported": self.reported_progress,
            },
        }


def create_fixture(directory: Path) -> tuple[Path, Path]:
    directory.mkdir(parents=True, exist_ok=True)
    core = directory / "tavily_proxy.db"
    sidecar = directory / "tavily_proxy-observability.db"
    # Leave both databases schema-empty so the service owns migration/bootstrap state.
    for path in (core, sidecar):
        with sqlite3.connect(path):
            pass
    core.chmod(0o600)
    sidecar.chmod(0o600)
    return core, sidecar


def archive_revision(root: Path, revision: str, destination: Path) -> str:
    resolved = subprocess.check_output(
        ["git", "-C", str(root), "rev-parse", "--verify", f"{revision}^{{commit}}"],
        text=True,
    ).strip()
    archive = subprocess.Popen(
        ["git", "-C", str(root), "archive", resolved], stdout=subprocess.PIPE
    )
    assert archive.stdout is not None
    with tarfile.open(fileobj=archive.stdout, mode="r|") as stream:
        stream.extractall(destination, filter="data")
    if archive.wait() != 0:
        raise RuntimeError(f"unable to archive revision {revision}")
    return resolved


def find_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.bind(("127.0.0.1", 0))
        return int(probe.getsockname()[1])


def wait_for_service(port: int, timeout_seconds: float) -> bool:
    deadline = time.monotonic() + timeout_seconds
    while time.monotonic() < deadline:
        try:
            connection = http.client.HTTPConnection("127.0.0.1", port, timeout=1)
            connection.request("GET", "/health")
            connection.getresponse().read()
            connection.close()
            return True
        except (OSError, http.client.HTTPException, TimeoutError):
            time.sleep(0.25)
    return False


def collect_phase_metrics(port: int, token: str) -> dict[str, dict[str, float]]:
    try:
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=3)
        connection.request("GET", "/metrics", headers={"Authorization": f"Bearer {token}"})
        response = connection.getresponse()
        body = response.read().decode("utf-8", errors="replace")
        connection.close()
    except (OSError, http.client.HTTPException, TimeoutError):
        return {}
    if response.status != 200:
        return {}
    values: dict[str, dict[str, float]] = {}
    for line in body.splitlines():
        if not line.startswith("hikari_http_request_duration_seconds_"):
            continue
        match = re.match(r"^hikari_http_request_duration_seconds_(count|sum)\{([^}]*)\}\s+([0-9.eE+-]+)$", line)
        if not match:
            continue
        metric_kind, labels_text, raw_value = match.groups()
        labels = dict(re.findall(r'(\w+)="([^"]*)"', labels_text))
        if any(labels.get(key) != value for key, value in PHASE_METRIC_SELECTOR.items()):
            continue
        phase = labels.get("phase")
        if phase is None:
            continue
        bucket = values.setdefault(phase, {"count": 0.0, "sum_seconds": 0.0})
        bucket["count" if metric_kind == "count" else "sum_seconds"] = float(raw_value)
    return values


def request_once(
    port: int,
    path: str,
    body: bytes,
    headers: dict[str, str],
    recorder: SampleRecorder,
    lane: str,
    method: str = "POST",
) -> None:
    started = time.monotonic()
    try:
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        connection.request(method, path, body=body, headers=headers)
        response = connection.getresponse()
        response_body = response.read()
        recorder.add_status(lane, response.status, (time.monotonic() - started) * 1000, response_body)
        connection.close()
    except (OSError, http.client.HTTPException, TimeoutError) as error:
        recorder.add_error(lane, error)


def run_load(
    port: int,
    warmup_seconds: int,
    duration_seconds: int,
    subjects: tuple[str, ...],
    tokens: tuple[str, ...],
    interval_seconds: float,
    on_measurement_start: Callable[[], dict[str, dict[str, float]]] | None = None,
) -> tuple[SampleRecorder, dict[str, dict[str, float]]]:
    recorder = SampleRecorder()
    payload = b'{"query":"synthetic","search_depth":"basic","max_results":1}'
    started = time.monotonic()
    deadline = started + warmup_seconds
    index = 0
    while time.monotonic() < deadline:
        request_once(
            port,
            "/api/tavily/search",
            payload,
            {
                "Content-Type": "application/json",
                "Authorization": f"Bearer {tokens[index % len(tokens)]}",
                "X-Perf-Subject": subjects[index % len(subjects)],
            },
            recorder,
            "warmup",
        )
        index += 1
    recorder = SampleRecorder()
    phase_baseline = on_measurement_start() if on_measurement_start else {}
    deadline = time.monotonic() + duration_seconds
    next_request = time.monotonic()
    while time.monotonic() < deadline:
        request_once(
            port,
            "/api/tavily/search",
            payload,
            {
                "Content-Type": "application/json",
                "Authorization": f"Bearer {tokens[index % len(tokens)]}",
                "X-Perf-Subject": subjects[index % len(subjects)],
            },
            recorder,
            "business",
        )
        if index % 4 == 0:
            request_once(
                port,
                "/health",
                b"{}",
                {"Content-Type": "application/json"},
                recorder,
                "unrelated",
                method="GET",
            )
        index += 1
        next_request += interval_seconds
        time.sleep(max(0, next_request - time.monotonic()))
    return recorder, phase_baseline


def bootstrap_business_tokens(port: int, labels: tuple[str, ...]) -> tuple[str, ...]:
    access_tokens: list[str] = []
    for label in labels:
        payload = json.dumps({"note": f"perf-local-{label}"}).encode("utf-8")
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        try:
            connection.request(
                "POST",
                "/api/tokens",
                body=payload,
                headers={"Content-Type": "application/json"},
            )
            response = connection.getresponse()
            body = response.read()
        except (OSError, http.client.HTTPException, TimeoutError) as error:
            raise RuntimeError("token_bootstrap_transport") from error
        finally:
            connection.close()
        if response.status != 201:
            raise RuntimeError(f"token_bootstrap_status_{response.status}")
        try:
            token = json.loads(body).get("token")
        except (ValueError, UnicodeDecodeError) as error:
            raise RuntimeError("token_bootstrap_invalid_json") from error
        if not isinstance(token, str) or not token:
            raise RuntimeError("token_bootstrap_missing_secret")
        access_tokens.append(token)
    return tuple(access_tokens)


def phase_delta(
    before: dict[str, dict[str, float]], after: dict[str, dict[str, float]]
) -> dict[str, dict[str, float]]:
    phases: dict[str, dict[str, float]] = {}
    for phase in sorted(set(before) | set(after)):
        current = after.get(phase, {})
        initial = before.get(phase, {})
        phases[phase] = {
            "count": max(0.0, current.get("count", 0.0) - initial.get("count", 0.0)),
            "sum_seconds": max(
                0.0,
                current.get("sum_seconds", 0.0) - initial.get("sum_seconds", 0.0),
            ),
        }
    return phases


def terminate_process(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is not None:
        return
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait(timeout=5)


def run_variant(
    root: Path,
    revision: str,
    variant: str,
    workdir: Path,
    mock: MockUpstream,
    args: argparse.Namespace,
    round_number: int,
    duration_seconds: int,
    log_directory: Path,
) -> dict[str, Any]:
    workdir.mkdir(parents=True, exist_ok=True)
    repo = workdir / f"{variant}-repo"
    repo.mkdir()
    resolved = archive_revision(root, revision, repo)
    core, sidecar = create_fixture(workdir / f"{variant}-fixture")
    port = find_free_port()
    metrics_port = find_free_port()
    metrics_token = "perf-metrics-token"
    metrics_token_path = workdir / "metrics.token"
    metrics_token_path.write_text(metrics_token + "\n", encoding="ascii")
    metrics_token_path.chmod(0o600)
    log_directory.mkdir(parents=True, exist_ok=True)
    if not args.service_command:
        return {
            "variant": variant,
            "round": round_number,
            "ref": resolved,
            "status": "unvalidated",
            "reason": "service_command_missing",
            "duration_seconds": duration_seconds,
            "request": None,
            "unrelated_cohort": None,
            "phases": {},
            "accepted_progress": {"responses": 0, "reported": 0},
            "cohort": {"subjects": list(args.subjects), "tokens": list(args.tokens)},
        }
    environment = dict(os.environ)
    environment.update(
        {
            "PERF_LOCAL_VARIANT": variant,
            "PERF_LOCAL_ROUND": str(round_number),
            "PERF_LOCAL_REPO": str(repo),
            "PERF_LOCAL_DB_PATH": str(core),
            "PERF_LOCAL_SIDECAR_DB_PATH": str(sidecar),
            "PERF_LOCAL_UPSTREAM_URL": mock.url,
            "PERF_LOCAL_PORT": str(port),
            "PERF_LOCAL_METRICS_PORT": str(metrics_port),
            "PERF_LOCAL_METRICS_ENABLED": "1" if args.metrics_enabled else "0",
            "PERF_LOCAL_SUBJECTS": ",".join(args.subjects),
            "PERF_LOCAL_TOKENS": ",".join(args.tokens),
            "PERF_LOCAL_RESOURCES": args.resources,
            "TAVILY_UPSTREAM": mock.url,
            "TAVILY_USAGE_BASE": mock.url,
            "PROXY_DB_PATH": str(core),
            "PROXY_BIND": "127.0.0.1",
            "PROXY_PORT": str(port),
            "OBSERVABILITY_ENABLED": "true" if args.metrics_enabled else "false",
            "OBSERVABILITY_ENVIRONMENT": "test",
            "OBSERVABILITY_INSTANCE": "local",
            "METRICS_BIND": f"127.0.0.1:{metrics_port}",
            "METRICS_TOKEN_FILE": str(metrics_token_path),
            "TAVILY_API_KEYS": "tvly-synthetic-test-key",
            "HA_MODE": "single",
            "DEV_OPEN_ADMIN": "true",
        }
    )
    command = args.service_command.format(
        repo=shlex.quote(str(repo)),
        db=shlex.quote(str(core)),
        sidecar=shlex.quote(str(sidecar)),
        port=port,
        upstream=shlex.quote(mock.url),
        variant=variant,
    )
    stdout_log = BoundedLogWriter(log_directory / "stdout.log")
    stderr_log = BoundedLogWriter(log_directory / "stderr.log")
    process = subprocess.Popen(
        command,
        cwd=repo,
        env=environment,
        shell=True,
        start_new_session=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    output_threads = [
        threading.Thread(
            target=drain_process_output,
            args=(process.stdout, stdout_log),
            daemon=True,
        ),
        threading.Thread(
            target=drain_process_output,
            args=(process.stderr, stderr_log),
            daemon=True,
        ),
    ]
    for thread in output_threads:
        thread.start()
    try:
        if not wait_for_service(port, args.readiness_timeout_seconds):
            return {
                "variant": variant,
                "round": round_number,
                "ref": resolved,
                "status": "unvalidated",
                "reason": "service_readiness_timeout",
                "duration_seconds": duration_seconds,
                "request": None,
                "unrelated_cohort": None,
                "phases": {},
                "accepted_progress": {"responses": 0, "reported": 0},
                "cohort": {"subjects": list(args.subjects), "tokens": list(args.tokens)},
            }
        access_tokens = tuple(args.tokens)
        if args.bootstrap_business_tokens:
            try:
                access_tokens = bootstrap_business_tokens(port, tuple(args.tokens))
            except RuntimeError:
                return {
                    "variant": variant,
                    "round": round_number,
                    "ref": resolved,
                    "status": "unvalidated",
                    "reason": "business_token_bootstrap_failed",
                    "duration_seconds": duration_seconds,
                    "request": None,
                    "unrelated_cohort": None,
                    "phases": {},
                    "accepted_progress": {"responses": 0, "reported": 0},
                    "cohort": {"subjects": list(args.subjects), "tokens": list(args.tokens)},
                }
        result, phase_baseline = run_load(
            port,
            args.warmup_seconds,
            duration_seconds,
            tuple(args.subjects),
            access_tokens,
            args.request_interval_seconds,
            on_measurement_start=lambda: collect_phase_metrics(metrics_port, metrics_token),
        )
        result_payload = result.result(duration_seconds)
        time.sleep(1.0)
        result_payload["phases"] = phase_delta(
            phase_baseline, collect_phase_metrics(metrics_port, metrics_token)
        )
        result = result_payload
        result.update(
            {
                "variant": variant,
                "round": round_number,
                "ref": resolved,
                "status": "measured",
                "cohort": {"subjects": list(args.subjects), "tokens": list(args.tokens)},
            }
        )
        return result
    finally:
        terminate_process(process)
        for thread in output_threads:
            thread.join(timeout=2)
        stdout_log.close()
        stderr_log.close()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline-ref", default=DEFAULT_BASELINE_REF)
    parser.add_argument("--candidate-ref", default="HEAD")
    parser.add_argument("--rounds", type=int, default=DEFAULT_ROUNDS)
    parser.add_argument("--warmup-seconds", type=int, default=DEFAULT_WARMUP_SECONDS)
    parser.add_argument("--duration-seconds", type=int, default=DEFAULT_DURATION_SECONDS)
    parser.add_argument("--extension-seconds", type=int, default=DEFAULT_EXTENSION_SECONDS)
    parser.add_argument("--mock-delay-ms", type=int, default=DEFAULT_MOCK_DELAY_MS)
    parser.add_argument("--min-samples", type=int, default=DEFAULT_MIN_SAMPLES)
    parser.add_argument("--request-interval-seconds", type=float, default=DEFAULT_REQUEST_INTERVAL_SECONDS)
    parser.add_argument("--readiness-timeout-seconds", type=float, default=60)
    parser.add_argument("--service-command", help="shell template; supports {repo}, {db}, {sidecar}, {port}, {upstream}, {variant}")
    parser.add_argument("--comparison-entry", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=Path("artifacts/perf-local-boundaries.json"))
    parser.add_argument(
        "--run-directory",
        type=Path,
        help="persistent directory for contract, checkpoints and bounded logs",
    )
    parser.add_argument("--resources", default=DEFAULT_RESOURCES)
    parser.add_argument("--subjects", nargs="+", default=list(DEFAULT_SUBJECTS))
    parser.add_argument("--tokens", nargs="+", default=list(DEFAULT_TOKENS))
    parser.add_argument("--metrics-enabled", action=argparse.BooleanOptionalAction, default=True)
    parser.add_argument(
        "--bootstrap-business-tokens",
        action=argparse.BooleanOptionalAction,
        default=False,
        help="create one temporary dev-open-admin access token per synthetic token label",
    )
    return parser.parse_args()


def validate_args(args: argparse.Namespace) -> None:
    if args.rounds < 1:
        raise SystemExit("--rounds must be positive")
    if args.warmup_seconds < 0 or args.duration_seconds < 1:
        raise SystemExit("warmup must be non-negative and duration must be positive")
    if args.duration_seconds > args.extension_seconds:
        raise SystemExit("duration cannot exceed extension duration")
    if args.mock_delay_ms != DEFAULT_MOCK_DELAY_MS:
        raise SystemExit("the local boundary contract fixes mock delay at 200ms")
    if not args.subjects or not args.tokens:
        raise SystemExit("subject and token distributions must be non-empty")
    if any(not re.fullmatch(r"subject-[a-z0-9-]+", value) for value in args.subjects):
        raise SystemExit("subjects must use synthetic subject-* values")
    if any(not re.fullmatch(r"token-[a-z0-9-]+", value) for value in args.tokens):
        raise SystemExit("tokens must use synthetic token-* values")
    if not args.comparison_entry.is_file():
        raise SystemExit(f"comparison entry does not exist: {args.comparison_entry}")


def create_run_directory(output: Path, requested: Path | None) -> Path:
    output.parent.mkdir(parents=True, exist_ok=True)
    if requested is None:
        stamp = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
        requested = output.parent / f"{output.stem}.run-{stamp}-{os.getpid()}"
    run_directory = requested.resolve()
    run_directory.mkdir(parents=True, exist_ok=False)
    return run_directory


def install_interrupt_handlers() -> dict[int, Any]:
    previous: dict[int, Any] = {}

    def handle(signum: int, _frame: Any) -> None:
        raise HarnessInterrupted(signal.Signals(signum).name)

    for signum in (signal.SIGINT, signal.SIGTERM):
        previous[signum] = signal.signal(signum, handle)
    return previous


def restore_interrupt_handlers(previous: dict[int, Any]) -> None:
    for signum, handler in previous.items():
        signal.signal(signum, handler)


def extension_allowed(comparison: dict[str, Any]) -> bool:
    """Only extend when samples or the fixed unrelated control are uncertain."""
    reasons = comparison.get("reasons", [])
    if not reasons:
        return False
    return all(
        reason.endswith("/request.samples_below_minimum")
        or reason.endswith("/control.samples_missing")
        or reason.endswith("/control/p95_regression_over_5_percent")
        for reason in reasons
    )


def run_paired_rounds(
    root: Path,
    workdir: Path,
    mock: MockUpstream,
    args: argparse.Namespace,
    duration_seconds: int,
    phase: str,
    checkpoint: CheckpointStore,
) -> list[dict[str, Any]]:
    paired_rounds: list[dict[str, Any]] = []
    for round_number in range(1, args.rounds + 1):
        round_dir = workdir / f"round-{round_number}"
        checkpoint.write_variant_started(
            phase, round_number, "baseline", args.baseline_ref, duration_seconds
        )
        try:
            baseline = run_variant(
                root,
                args.baseline_ref,
                "baseline",
                round_dir,
                mock,
                args,
                round_number,
                duration_seconds,
                checkpoint.log_directory(phase, round_number, "baseline"),
            )
        except HarnessInterrupted:
            raise
        except Exception as error:  # keep a precise, bounded failure checkpoint
            baseline = variant_exception_result(
                "baseline", round_number, args.baseline_ref, args, error
            )
        checkpoint.write_variant_result(phase, round_number, "baseline", baseline)

        checkpoint.write_variant_started(
            phase, round_number, "candidate", args.candidate_ref, duration_seconds
        )
        try:
            candidate = run_variant(
                root,
                args.candidate_ref,
                "candidate",
                round_dir,
                mock,
                args,
                round_number,
                duration_seconds,
                checkpoint.log_directory(phase, round_number, "candidate"),
            )
        except HarnessInterrupted:
            raise
        except Exception as error:  # keep a precise, bounded failure checkpoint
            candidate = variant_exception_result(
                "candidate", round_number, args.candidate_ref, args, error
            )
        checkpoint.write_variant_result(phase, round_number, "candidate", candidate)

        round_record = {"round": round_number, "baseline": baseline, "candidate": candidate}
        paired_rounds.append(round_record)
        round_comparison = compare_rounds([round_record], args.min_samples)["rounds"][0]
        checkpoint.write_pair(phase, round_number, baseline, candidate, round_comparison)
    return paired_rounds


def p95(values: list[float] | None) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, int(len(ordered) * 0.95))]


def cohort_key(result: dict[str, Any]) -> str | None:
    cohort = result.get("cohort")
    if not isinstance(cohort, dict):
        return None
    return json.dumps(cohort, sort_keys=True, separators=(",", ":"))


def measurement_view(result: dict[str, Any]) -> dict[str, Any]:
    return {
        "ref": result.get("ref"),
        "status": result.get("status"),
        "reason": result.get("reason"),
        "duration_seconds": result.get("duration_seconds"),
        "request": result.get("request"),
        "unrelated_cohort": result.get("unrelated_cohort"),
        "statuses": result.get("statuses", {}),
        "errors": result.get("errors", {}),
        "phases": result.get("phases", {}),
        "cohort": result.get("cohort"),
        "accepted_progress": result.get("accepted_progress"),
    }


def measurement_reasons(
    round_number: int,
    role: str,
    result: dict[str, Any],
    min_samples: int,
) -> list[str]:
    prefix = f"round-{round_number}/{role}"
    reasons: list[str] = []
    if result.get("status") != "measured":
        reasons.append(f"{prefix}/status/{result.get('reason') or 'not_measured'}")

    request = result.get("request")
    if not isinstance(request, dict):
        reasons.append(f"{prefix}/request.missing")
    else:
        if request.get("samples", 0) < min_samples:
            reasons.append(f"{prefix}/request.samples_below_minimum")
        if request.get("p95_ms") is None:
            reasons.append(f"{prefix}/request.p95_missing")

    control = result.get("unrelated_cohort")
    if not isinstance(control, dict):
        reasons.append(f"{prefix}/control.missing")
    else:
        if control.get("samples", 0) <= 0:
            reasons.append(f"{prefix}/control.samples_missing")
        elif control.get("p95_ms") is None:
            reasons.append(f"{prefix}/control.p95_missing")
    if cohort_key(result) is None:
        reasons.append(f"{prefix}/cohort.missing")
    return reasons


def compare_round(
    round_record: dict[str, Any],
    min_samples: int,
    expected_cohort: str | None = None,
) -> dict[str, Any]:
    round_number = int(round_record["round"])
    baseline = round_record["baseline"]
    candidate = round_record["candidate"]
    reasons = measurement_reasons(round_number, "baseline", baseline, min_samples)
    reasons.extend(measurement_reasons(round_number, "candidate", candidate, min_samples))
    baseline_cohort = cohort_key(baseline)
    candidate_cohort = cohort_key(candidate)
    if baseline_cohort is not None and candidate_cohort is not None:
        if baseline_cohort != candidate_cohort:
            reasons.append(f"round-{round_number}/cohort/mismatch")
        if expected_cohort is not None and baseline_cohort != expected_cohort:
            reasons.append(f"round-{round_number}/cohort/changed_across_rounds")

    if reasons:
        return {
            "round": round_number,
            "status": "unvalidated",
            "comparable": False,
            "reasons": reasons,
            "baseline": measurement_view(baseline),
            "candidate": measurement_view(candidate),
            "affected": None,
            "control": None,
        }

    baseline_request = baseline["request"]
    candidate_request = candidate["request"]
    baseline_control = baseline["unrelated_cohort"]
    candidate_control = candidate["unrelated_cohort"]
    baseline_request_p95 = baseline_request["p95_ms"]
    candidate_request_p95 = candidate_request["p95_ms"]
    baseline_control_p95 = baseline_control["p95_ms"]
    candidate_control_p95 = candidate_control["p95_ms"]
    if baseline_control_p95 <= 0:
        reasons.append(f"round-{round_number}/control/p95_non_positive")

    affected_regression = (
        candidate_request_p95 / baseline_request_p95 - 1
        if baseline_request_p95 > 0
        else None
    )
    control_regression = (
        candidate_control_p95 / baseline_control_p95 - 1
        if baseline_control_p95 > 0
        else None
    )
    if control_regression is None or control_regression > 0.05:
        reasons.append(f"round-{round_number}/control/p95_regression_over_5_percent")

    return {
        "round": round_number,
        "status": "unvalidated" if reasons else "validated",
        "comparable": not reasons,
        "reasons": reasons,
        "baseline": measurement_view(baseline),
        "candidate": measurement_view(candidate),
        "affected": {
            "baseline_p95_ms": baseline_request_p95,
            "candidate_p95_ms": candidate_request_p95,
            "p95_regression_ratio": affected_regression,
        },
        "control": {
            "baseline_p95_ms": baseline_control_p95,
            "candidate_p95_ms": candidate_control_p95,
            "p95_regression_ratio": control_regression,
            "stable": not reasons,
        },
    }


def compare_rounds(rounds: list[dict[str, Any]], min_samples: int) -> dict[str, Any]:
    if not rounds:
        return {
            "status": "unvalidated",
            "comparable": False,
            "reasons": ["run/rounds_missing"],
            "rounds": [],
            "summary": {"p95_is_descriptive_only": True},
        }

    comparisons: list[dict[str, Any]] = []
    reasons: list[str] = []
    expected_cohort: str | None = None
    for round_record in rounds:
        comparison = compare_round(round_record, min_samples, expected_cohort)
        comparisons.append(comparison)
        for reason in comparison["reasons"]:
            if reason not in reasons:
                reasons.append(reason)
        if expected_cohort is None:
            expected_cohort = cohort_key(round_record["baseline"])

    affected_baseline = [
        item["affected"]["baseline_p95_ms"]
        for item in comparisons
        if item["affected"] is not None
    ]
    affected_candidate = [
        item["affected"]["candidate_p95_ms"]
        for item in comparisons
        if item["affected"] is not None
    ]
    control_baseline = [
        item["control"]["baseline_p95_ms"]
        for item in comparisons
        if item["control"] is not None
    ]
    control_candidate = [
        item["control"]["candidate_p95_ms"]
        for item in comparisons
        if item["control"] is not None
    ]
    summary_baseline = p95(affected_baseline)
    summary_candidate = p95(affected_candidate)
    summary_control_baseline = p95(control_baseline)
    summary_control_candidate = p95(control_candidate)
    summary = {
        "p95_is_descriptive_only": True,
        "baseline_request_p95_ms": summary_baseline,
        "candidate_request_p95_ms": summary_candidate,
        "baseline_unrelated_p95_ms": summary_control_baseline,
        "candidate_unrelated_p95_ms": summary_control_candidate,
        "p95_regression_ratio": (
            summary_candidate / summary_baseline - 1
            if summary_baseline and summary_candidate is not None
            else None
        ),
        "unrelated_p95_regression_ratio": (
            summary_control_candidate / summary_control_baseline - 1
            if summary_control_baseline and summary_control_candidate is not None
            else None
        ),
    }
    status = "validated"
    if reasons:
        status = "unvalidated"
    elif any(item["status"] == "failed" for item in comparisons):
        status = "failed"
    return {
        "status": status,
        "comparable": not reasons,
        "reasons": reasons,
        "rounds": comparisons,
        "summary": summary,
    }


def variant_exception_result(
    variant: str,
    round_number: int,
    ref: str,
    args: argparse.Namespace,
    error: Exception,
) -> dict[str, Any]:
    return {
        "variant": variant,
        "round": round_number,
        "ref": ref,
        "status": "unvalidated",
        "reason": "variant_exception",
        "error_type": type(error).__name__,
        "duration_seconds": 0,
        "request": None,
        "unrelated_cohort": None,
        "statuses": {},
        "errors": {},
        "phases": {},
        "accepted_progress": {"responses": 0, "reported": 0},
        "cohort": {"subjects": list(args.subjects), "tokens": list(args.tokens)},
    }


def main() -> int:
    args = parse_args()
    validate_args(args)
    root = Path(__file__).resolve().parents[1]
    output = args.output.resolve()
    run_directory = create_run_directory(output, args.run_directory)
    contract = {
        "baseline_ref": args.baseline_ref,
        "candidate_ref": args.candidate_ref,
        "warmup_seconds": args.warmup_seconds,
        "duration_seconds": args.duration_seconds,
        "rounds": args.rounds,
        "mock_delay_ms": args.mock_delay_ms,
        "resources": args.resources,
        "subjects": args.subjects,
        "tokens": args.tokens,
        "bootstrap_business_tokens": args.bootstrap_business_tokens,
        "metrics_enabled": args.metrics_enabled,
        "comparison_entry": str(args.comparison_entry.resolve()),
        "upstream_kind": "loopback-synthetic-mock",
        "phase_metric_selector": PHASE_METRIC_SELECTOR,
        "run_directory": str(run_directory),
        "checkpoint_schema": 1,
    }
    checkpoint = CheckpointStore(run_directory, contract)
    evidence: dict[str, Any] = {
        "contract": contract,
        "rounds": [],
        "comparison": None,
        "status": "running",
        "checkpoint": {
            "run_directory": str(run_directory),
            "contract": str(run_directory / "contract.json"),
            "variants": str(checkpoint.variant_root),
            "pairs": str(checkpoint.pair_root),
            "logs": str(checkpoint.log_root),
            "progress": str(run_directory / "progress.json"),
        },
    }
    previous_handlers = install_interrupt_handlers()
    exit_code = 0
    try:
        with tempfile.TemporaryDirectory(prefix="perf-local-boundaries-") as temporary:
            workdir = Path(temporary)
            with MockUpstream(args.mock_delay_ms) as mock:
                evidence["rounds"] = run_paired_rounds(
                    root,
                    workdir / "initial",
                    mock,
                    args,
                    args.duration_seconds,
                    "initial",
                    checkpoint,
                )
                comparison = compare_rounds(evidence["rounds"], args.min_samples)
                evidence["comparison"] = comparison
                evidence["status"] = comparison["status"]
                if comparison["status"] == "unvalidated" and args.duration_seconds < args.extension_seconds:
                    if args.service_command and extension_allowed(comparison):
                        extension_rounds = run_paired_rounds(
                            root,
                            workdir / "extension",
                            mock,
                            args,
                            args.extension_seconds,
                            "extension",
                            checkpoint,
                        )
                        extension_comparison = compare_rounds(
                            extension_rounds, args.min_samples
                        )
                        evidence["extension"] = {
                            "status": extension_comparison["status"],
                            "duration_seconds": args.extension_seconds,
                            "reason": "sample_or_control_uncertainty",
                            "initial_reasons": comparison["reasons"],
                            "rounds": extension_rounds,
                            "comparison": extension_comparison,
                        }
                        if extension_comparison["status"] != "unvalidated":
                            evidence["comparison"] = extension_comparison
                            evidence["status"] = extension_comparison["status"]
                    elif not args.service_command:
                        evidence["extension"] = {
                            "status": "unavailable",
                            "duration_seconds": args.extension_seconds,
                            "reason": "service_command_missing",
                            "initial_reasons": comparison["reasons"],
                        }
                    else:
                        evidence["extension"] = {
                            "status": "not_allowed",
                            "duration_seconds": args.extension_seconds,
                            "reason": "non_extendable_failure",
                            "initial_reasons": comparison["reasons"],
                        }
                elif comparison["status"] == "unvalidated":
                    evidence["extension"] = {
                        "status": "exhausted",
                        "duration_seconds": args.extension_seconds,
                        "reason": "extension_window_unavailable",
                        "initial_reasons": comparison["reasons"],
                    }
                else:
                    evidence["extension"] = {"status": "not_needed"}
        checkpoint.mark_finished(evidence["status"])
    except HarnessInterrupted as error:
        checkpoint.mark_interrupted(str(error))
        evidence["status"] = "interrupted"
        evidence["interruption"] = {"reason": str(error)}
        exit_code = 130
    except KeyboardInterrupt:
        checkpoint.mark_interrupted("SIGINT")
        evidence["status"] = "interrupted"
        evidence["interruption"] = {"reason": "SIGINT"}
        exit_code = 130
    except Exception as error:
        reason = f"harness_exception:{type(error).__name__}"
        checkpoint.mark_interrupted(reason)
        evidence["status"] = "unvalidated"
        evidence["error_type"] = type(error).__name__
        evidence["error_reason"] = reason
        exit_code = 2
    finally:
        restore_interrupt_handlers(previous_handlers)
    atomic_write_json(output, evidence)
    print(json.dumps(evidence, indent=2, sort_keys=True))
    if exit_code:
        return exit_code
    return 1 if evidence["status"] == "failed" else 0


if __name__ == "__main__":
    raise SystemExit(main())
