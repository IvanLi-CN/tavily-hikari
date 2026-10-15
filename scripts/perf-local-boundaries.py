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
from typing import Any


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
    with sqlite3.connect(core) as connection:
        connection.execute(
            "CREATE TABLE perf_local_fixture (id INTEGER PRIMARY KEY, kind TEXT NOT NULL)"
        )
        connection.execute("INSERT INTO perf_local_fixture(kind) VALUES ('synthetic')")
    with sqlite3.connect(sidecar) as connection:
        connection.execute(
            "CREATE TABLE perf_local_fixture (id INTEGER PRIMARY KEY, kind TEXT NOT NULL)"
        )
        connection.execute("INSERT INTO perf_local_fixture(kind) VALUES ('synthetic')")
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
) -> None:
    started = time.monotonic()
    try:
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        connection.request("POST", path, body=body, headers=headers)
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
) -> SampleRecorder:
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
            )
        index += 1
        next_request += interval_seconds
        time.sleep(max(0, next_request - time.monotonic()))
    return recorder


def terminate_process(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is not None:
        return
    process.send_signal(signal.SIGTERM)
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()
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
    if not args.service_command:
        return {
            "variant": variant,
            "round": round_number,
            "ref": resolved,
            "status": "unvalidated",
            "reason": "service_command_missing",
            "request": None,
            "unrelated_cohort": None,
            "phases": {},
            "accepted_progress": {"responses": 0, "reported": 0},
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
    process = subprocess.Popen(
        command,
        cwd=repo,
        env=environment,
        shell=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    try:
        if not wait_for_service(port, args.readiness_timeout_seconds):
            return {
                "variant": variant,
                "round": round_number,
                "ref": resolved,
                "status": "unvalidated",
                "reason": "service_readiness_timeout",
                "request": None,
                "unrelated_cohort": None,
                "phases": {},
                "accepted_progress": {"responses": 0, "reported": 0},
            }
        result = run_load(
            port,
            args.warmup_seconds,
            duration_seconds,
            tuple(args.subjects),
            tuple(args.tokens),
            args.request_interval_seconds,
        )
        result_payload = result.result(duration_seconds)
        result_payload["phases"] = collect_phase_metrics(metrics_port, metrics_token)
        result = result_payload
        result.update({"variant": variant, "round": round_number, "ref": resolved, "status": "measured"})
        return result
    finally:
        terminate_process(process)


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
    parser.add_argument("--resources", default=DEFAULT_RESOURCES)
    parser.add_argument("--subjects", nargs="+", default=list(DEFAULT_SUBJECTS))
    parser.add_argument("--tokens", nargs="+", default=list(DEFAULT_TOKENS))
    parser.add_argument("--metrics-enabled", action=argparse.BooleanOptionalAction, default=True)
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


def run_paired_rounds(
    root: Path,
    workdir: Path,
    mock: MockUpstream,
    args: argparse.Namespace,
    duration_seconds: int,
) -> list[dict[str, Any]]:
    paired_rounds: list[dict[str, Any]] = []
    for round_number in range(1, args.rounds + 1):
        round_dir = workdir / f"round-{round_number}"
        paired_rounds.append(
            {
                "round": round_number,
                "baseline": run_variant(
                    root,
                    args.baseline_ref,
                    "baseline",
                    round_dir,
                    mock,
                    args,
                    round_number,
                    duration_seconds,
                ),
                "candidate": run_variant(
                    root,
                    args.candidate_ref,
                    "candidate",
                    round_dir,
                    mock,
                    args,
                    round_number,
                    duration_seconds,
                ),
            }
        )
    return paired_rounds


def p95(values: list[float] | None) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, int(len(ordered) * 0.95))]


def stable_measurements(results: list[dict[str, Any]], min_samples: int) -> bool:
    measured = [result for result in results if result.get("status") == "measured"]
    return len(measured) == len(results) and all(
        (result.get("request") or {}).get("samples", 0) >= min_samples for result in measured
    )


def compare_rounds(rounds: list[dict[str, Any]], min_samples: int) -> dict[str, Any]:
    baseline = [item["baseline"] for item in rounds]
    candidate = [item["candidate"] for item in rounds]
    unrelated_measured = all(
        (item.get("unrelated_cohort") or {}).get("samples", 0) > 0
        for item in baseline + candidate
    )
    comparable = stable_measurements(baseline + candidate, min_samples) and unrelated_measured
    baseline_p95 = [item["request"]["p95_ms"] for item in baseline if item.get("request")]
    candidate_p95 = [item["request"]["p95_ms"] for item in candidate if item.get("request")]
    baseline_unrelated = [
        item["unrelated_cohort"]["p95_ms"] for item in baseline if item.get("unrelated_cohort")
    ]
    candidate_unrelated = [
        item["unrelated_cohort"]["p95_ms"] for item in candidate if item.get("unrelated_cohort")
    ]
    result: dict[str, Any] = {
        "status": "unvalidated" if not comparable else "validated",
        "comparable": comparable,
        "baseline_request_p95_ms": p95(baseline_p95),
        "candidate_request_p95_ms": p95(candidate_p95),
        "baseline_unrelated_p95_ms": p95(baseline_unrelated),
        "candidate_unrelated_p95_ms": p95(candidate_unrelated),
        "stable_unrelated_cohort": False,
        "p95_regression_ratio": None,
        "unrelated_p95_regression_ratio": None,
    }
    if not comparable or not baseline_p95 or not candidate_p95:
        return result
    base = result["baseline_request_p95_ms"]
    cand = result["candidate_request_p95_ms"]
    result["p95_regression_ratio"] = (cand / base - 1) if base else None
    if baseline_unrelated and candidate_unrelated and result["baseline_unrelated_p95_ms"]:
        unrelated_base = result["baseline_unrelated_p95_ms"]
        unrelated_candidate = result["candidate_unrelated_p95_ms"]
        result["unrelated_p95_regression_ratio"] = unrelated_candidate / unrelated_base - 1
        baseline_stable = all(
            value >= unrelated_base * 0.95 and value <= unrelated_base * 1.05
            for value in baseline_unrelated
        )
        candidate_center = result["candidate_unrelated_p95_ms"]
        candidate_stable = all(
            candidate_center is not None
            and candidate_center * 0.95 <= value <= candidate_center * 1.05
            for value in candidate_unrelated
        )
        result["stable_unrelated_cohort"] = baseline_stable and candidate_stable
    result["status"] = (
        "failed"
        if result["stable_unrelated_cohort"] and result["unrelated_p95_regression_ratio"] > 0.05
        else "validated"
    )
    return result


def main() -> int:
    args = parse_args()
    validate_args(args)
    root = Path(__file__).resolve().parents[1]
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    evidence: dict[str, Any] = {
        "contract": {
            "baseline_ref": args.baseline_ref,
            "candidate_ref": args.candidate_ref,
            "warmup_seconds": args.warmup_seconds,
            "duration_seconds": args.duration_seconds,
            "rounds": args.rounds,
            "mock_delay_ms": args.mock_delay_ms,
            "resources": args.resources,
            "subjects": args.subjects,
            "tokens": args.tokens,
            "metrics_enabled": args.metrics_enabled,
            "comparison_entry": str(args.comparison_entry.resolve()),
            "upstream_kind": "loopback-synthetic-mock",
        },
        "rounds": [],
        "comparison": None,
        "status": "unvalidated",
    }
    with tempfile.TemporaryDirectory(prefix="perf-local-boundaries-") as temporary:
        workdir = Path(temporary)
        with MockUpstream(args.mock_delay_ms) as mock:
            evidence["rounds"] = run_paired_rounds(
                root, workdir / "initial", mock, args, args.duration_seconds
            )
            comparison = compare_rounds(evidence["rounds"], args.min_samples)
            evidence["comparison"] = comparison
            evidence["status"] = comparison["status"]
            if comparison["status"] == "unvalidated" and args.service_command and args.duration_seconds < args.extension_seconds:
                extension_rounds = run_paired_rounds(
                    root, workdir / "extension", mock, args, args.extension_seconds
                )
                extension_comparison = compare_rounds(extension_rounds, args.min_samples)
                evidence["extension"] = {
                    "status": extension_comparison["status"],
                    "duration_seconds": args.extension_seconds,
                    "reason": "noise_or_insufficient_samples",
                    "rounds": extension_rounds,
                    "comparison": extension_comparison,
                }
                if extension_comparison["status"] != "unvalidated":
                    evidence["comparison"] = extension_comparison
                    evidence["status"] = extension_comparison["status"]
            elif comparison["status"] == "unvalidated" and args.duration_seconds < args.extension_seconds:
                evidence["extension"] = {
                    "status": "unavailable",
                    "duration_seconds": args.extension_seconds,
                    "reason": "service_command_missing",
                }
            elif comparison["status"] == "unvalidated":
                evidence["extension"] = {
                    "status": "exhausted",
                    "duration_seconds": args.extension_seconds,
                    "reason": "noise_or_insufficient_samples",
                }
            else:
                evidence["extension"] = {"status": "not_needed"}
    output.write_text(json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps(evidence, indent=2, sort_keys=True))
    return 1 if evidence["status"] == "failed" else 0


if __name__ == "__main__":
    raise SystemExit(main())
