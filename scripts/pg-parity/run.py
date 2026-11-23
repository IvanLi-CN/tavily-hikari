#!/usr/bin/env python3
"""Differential replay: one binary, one deterministic load, two storage backends.

Run the same traffic against a SQLite-backed instance and a PostgreSQL-backed
instance, export the stored facts from each backend, and compare them.

Exit codes:
  0  the two backends agree
  1  a substantive difference was found
  2  the environment could not produce a valid replay

Nothing here talks to the production Tavily endpoint or a production database.
The upstream is the checked-in HTTP stub under `tests/performance_recovery/`.
"""

from __future__ import annotations

import argparse
import ipaddress
import json
import os
import re
import shutil
import sqlite3
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any
from urllib.parse import quote, urlsplit

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent.parent
sys.path.insert(0, str(SCRIPT_DIR))

import facts as facts_mod  # noqa: E402
import traffic as traffic_mod  # noqa: E402


MOCK_UPSTREAM = REPO_ROOT / "tests" / "performance_recovery" / "mock_upstream.py"
PARITY_KEY = "tvly-parity-local"
DEFAULT_SEED = 20261123
DEFAULT_MOCK_PORT = 19001
DEFAULT_BACKEND_PORT = 58187
DEFAULT_SQLITE_PORT = 58188

IGNORED_TOP_LEVEL = {"meta"}
IGNORED_KEY_RE = re.compile(r"(^id$|_id$|_at$|^created_at$|^updated_at$|^exported_at_unix$)")
TRIVIAL_RELATIVE_DELTA = 1e-9


class EnvError(RuntimeError):
    """The environment cannot produce a valid replay (exit code 2)."""


# ---------------------------------------------------------------------------
# Small helpers
# ---------------------------------------------------------------------------


def redact_url(url: str) -> str:
    parsed = urlsplit(url)
    host = parsed.hostname or "?"
    port = "" if parsed.port is None else f":{parsed.port}"
    database = parsed.path.lstrip("/") or "?"
    return f"{parsed.scheme}://{host}{port}/{database}"


def validate_pg_url(pg_url: str, allow_non_local: bool) -> None:
    parsed = urlsplit(pg_url)
    if parsed.scheme not in ("postgres", "postgresql"):
        raise EnvError(
            f"--pg-url must use postgres:// or postgresql://, got {parsed.scheme!r}"
        )
    if not parsed.path.strip("/"):
        raise EnvError("--pg-url must name a database")
    host = parsed.hostname
    if not host:
        raise EnvError("--pg-url must name a host (or use a local socket form)")
    try:
        addresses = {
            info[4][0]
            for info in socket.getaddrinfo(host, parsed.port or 5432, type=socket.SOCK_STREAM)
        }
    except socket.gaierror as error:
        raise EnvError(f"--pg-url host does not resolve: {host}: {error}") from error
    global_addresses = sorted(
        address
        for address in addresses
        if ipaddress.ip_address(address.split("%", 1)[0]).is_global
    )
    if global_addresses and not allow_non_local:
        raise EnvError(
            "refusing to reset schemas on a globally routable PostgreSQL host "
            f"({host} -> {', '.join(global_addresses)}); pass "
            "--allow-non-local-pg only for a dedicated test database"
        )


def port_is_free(port: int) -> bool:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.settimeout(0.5)
        return sock.connect_ex(("127.0.0.1", port)) != 0


def require_free_port(port: int, label: str) -> None:
    if not port_is_free(port):
        raise EnvError(f"{label} port {port} is already in use")


def wait_for_tcp(port: int, proc: subprocess.Popen[bytes], log_path: Path, timeout: float) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            raise EnvError(
                f"process exited before listening on {port} (see {log_path})"
            )
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.settimeout(0.5)
            if sock.connect_ex(("127.0.0.1", port)) == 0:
                return
        time.sleep(0.1)
    raise EnvError(f"timed out waiting for port {port} (see {log_path})")


def wait_for_health(
    port: int, proc: subprocess.Popen[bytes], log_path: Path, timeout: float
) -> None:
    deadline = time.monotonic() + timeout
    url = f"http://127.0.0.1:{port}/health"
    last_error: str | None = None
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            raise EnvError(
                f"backend exited before /health became ready (see {log_path})"
            )
        try:
            with urllib.request.urlopen(url, timeout=2) as response:
                if response.status == 200:
                    return
                last_error = f"status {response.status}"
        except (urllib.error.URLError, TimeoutError, OSError) as error:
            last_error = str(error)
        time.sleep(0.2)
    raise EnvError(f"/health did not become ready: {last_error} (see {log_path})")


def stop_process(proc: subprocess.Popen[bytes], name: str, log_path: Path) -> None:
    if proc.poll() is not None:
        return
    proc.terminate()
    try:
        proc.wait(timeout=15)
    except subprocess.TimeoutExpired:
        print(f"warning: {name} ignored SIGTERM; killing (see {log_path})", file=sys.stderr)
        proc.kill()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            print(f"warning: {name} did not exit after SIGKILL", file=sys.stderr)


def _http_request(
    method: str,
    url: str,
    timeout: float = 30.0,
    *,
    payload: dict[str, Any] | None = None,
) -> tuple[int, bytes]:
    data = None
    headers = {}
    if payload is not None:
        data = json.dumps(payload).encode()
        headers["content-type"] = "application/json"
    request = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return response.status, response.read()
    except urllib.error.HTTPError as error:
        return error.code, error.read()


def pin_key_quota(base_url: str) -> None:
    """Force one manual quota sync so the balance fact cannot depend on scheduler timing."""
    status, body = _http_request("GET", f"{base_url}/api/keys?per_page=200")
    if status != 200:
        raise EnvError(f"failed to list API keys: status={status}")
    try:
        payload = json.loads(body)
    except json.JSONDecodeError as error:
        raise EnvError(f"API key list was not JSON: {error}") from error
    items = payload.get("items", []) if isinstance(payload, dict) else None
    if not isinstance(items, list):
        raise EnvError("API key list response did not contain an items array")
    key_ids = [
        item["id"]
        for item in items
        if isinstance(item, dict) and item.get("id")
    ]
    if not key_ids:
        raise EnvError("no API keys were registered; cannot pin the balance fact")
    for key_id in key_ids:
        status, body = _http_request(
            "POST",
            f"{base_url}/api/keys/{quote(key_id, safe='')}/sync-usage",
        )
        if status != 204:
            detail = body[:200].decode(errors="replace")
            if "FOREIGN KEY constraint failed" in detail:
                raise EnvError(
                    "manual quota sync writes through the SQLite lanes while key creation "
                    "already ran on PostgreSQL, so a partially ported store cannot pin the "
                    "key-pool balance. Re-run with --skip-manual-quota-sync before the "
                    f"cutover, or finish the port and keep the default. key={key_id} "
                    f"status={status} body={detail}"
                )
            raise EnvError(
                f"manual quota sync failed for key {key_id}: status={status} body={detail}"
            )


def raise_rate_limits(base_url: str) -> None:
    """Lift the per-window request rate limit so the seeded plan produces no throttling.

    429 outcomes depend on the wall-clock window, so leaving them in makes two runs of the
    same plan non-comparable. The limit is a supported admin setting; the fixture raises it
    on its own private database and never on a shared environment.
    """
    status, body = _http_request("GET", f"{base_url}/api/settings")
    if status != 200:
        raise EnvError(f"failed to read system settings: status={status}")
    try:
        current = json.loads(body)
    except json.JSONDecodeError as error:
        raise EnvError(f"system settings were not JSON: {error}") from error
    affinity = current.get("mcpSessionAffinityKeyCount", 4) if isinstance(current, dict) else 4
    payload = {
        "requestRateLimit": 1_000_000,
        "mcpSessionAffinityKeyCount": affinity,
    }
    status, body = _http_request(
        "PUT",
        f"{base_url}/api/settings/system",
        payload=payload,
    )
    if status != 200:
        raise EnvError(
            f"failed to raise the request rate limit: status={status} "
            f"body={body[:200].decode(errors='replace')}"
        )


def wait_for_request_logs(base_url: str, timeout: float, *, quiet_secs: float = 5.0) -> int:
    """Wait until deferred audit writes stop growing.

    Rebalance control-plane rows are persisted by a debounced background writer, and only
    some traffic classes produce a request log (throttled and upstream-failed calls do not),
    so an exact row target cannot be derived from the traffic plan. The fixture instead waits
    for a quiet window and then exports, refusing to compare a still-draining state.
    """
    deadline = time.monotonic() + timeout
    last_total: int | None = None
    stable_since: float | None = None
    while time.monotonic() < deadline:
        status, body = _http_request("GET", f"{base_url}/api/logs?per_page=1")
        if status == 200:
            try:
                payload = json.loads(body)
                total = int(payload.get("total", -1)) if isinstance(payload, dict) else None
            except (json.JSONDecodeError, TypeError, ValueError):
                total = None
            if total is not None:
                if total != last_total:
                    last_total = total
                    stable_since = time.monotonic()
                elif stable_since is not None and time.monotonic() - stable_since >= quiet_secs:
                    return total
        time.sleep(0.25)
    raise EnvError(
        f"request_logs did not settle within {timeout:.0f}s "
        f"(last total: {last_total}); deferred audit writes are still arriving"
    )


def tail(path: Path, lines: int = 20) -> str:
    try:
        content = path.read_text(errors="replace").splitlines()
    except OSError:
        return ""
    return "\n".join(content[-lines:])


def psql(pg_url: str, sql: str) -> subprocess.CompletedProcess[str]:
    command = ["psql", pg_url, "-X", "-q", "-v", "ON_ERROR_STOP=1", "-c", sql]
    try:
        return subprocess.run(
            command,
            capture_output=True,
            text=True,
            timeout=120,
            check=False,
            env={**os.environ, "PGAPPNAME": "pg-parity-run", "PGCONNECT_TIMEOUT": "10"},
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise EnvError(f"psql failed to run: {error}") from error


def reset_pg_schemas(pg_url: str) -> None:
    result = psql(
        pg_url,
        "DROP SCHEMA IF EXISTS core CASCADE; "
        "DROP SCHEMA IF EXISTS observability CASCADE; "
        # sqlx keeps its ledger in the default schema; leaving it behind makes
        # the next startup believe migrations were already applied.
        "DROP TABLE IF EXISTS public._sqlx_migrations CASCADE;",
    )
    if result.returncode != 0:
        raise EnvError(
            "failed to reset core/observability schemas: " + result.stderr.strip()
        )


# ---------------------------------------------------------------------------
# Backend lifecycle
# ---------------------------------------------------------------------------


def start_mock_upstream(work_dir: Path, port: int) -> tuple[subprocess.Popen[bytes], Path, Any]:
    if not MOCK_UPSTREAM.is_file():
        raise EnvError(f"mock upstream is missing: {MOCK_UPSTREAM}")
    require_free_port(port, "mock upstream")
    log_path = work_dir / "mock-upstream.log"
    log_file = log_path.open("wb")
    proc = subprocess.Popen(
        [
            sys.executable,
            str(MOCK_UPSTREAM),
            "--bind",
            "127.0.0.1",
            "--port",
            str(port),
        ],
        stdout=log_file,
        stderr=subprocess.STDOUT,
    )
    try:
        wait_for_tcp(port, proc, log_path, timeout=20)
    except EnvError:
        stop_process(proc, "mock upstream", log_path)
        log_file.close()
        raise
    return proc, log_path, log_file


def verify_postgres_serves(
    port: int,
    pg_url: str | None,
    proc: subprocess.Popen[bytes],
    log_path: Path,
    log_file: Any,
) -> None:
    """Fail with an environment error unless a write really lands in PostgreSQL."""

    if not pg_url:
        stop_process(proc, "postgres backend", log_path)
        log_file.close()
        raise EnvError("postgres backend requires --pg-url")

    request = urllib.request.Request(
        f"http://127.0.0.1:{port}/api/tokens",
        data=json.dumps({"note": f"parity-probe-{os.getpid()}"}).encode(),
        headers={"content-type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            status = response.status
    except urllib.error.HTTPError as error:
        status = error.code
    except OSError as error:
        status = None
        probe_error = str(error)
    else:
        probe_error = ""
    if status not in (200, 201):
        stop_process(proc, "postgres backend", log_path)
        log_file.close()
        detail = f": {probe_error}" if probe_error else ""
        raise EnvError(f"postgres backend rejected a token probe (status {status}{detail})")

    try:
        result = subprocess.run(
            ["psql", pg_url, "-At", "-c", "select count(*) from core.auth_tokens"],
            capture_output=True,
            text=True,
            timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        stop_process(proc, "postgres backend", log_path)
        log_file.close()
        raise EnvError(f"psql probe failed: {error}") from error
    if result.returncode != 0:
        stop_process(proc, "postgres backend", log_path)
        log_file.close()
        raise EnvError(f"psql probe failed: {result.stderr.strip()}")
    try:
        rows = int(result.stdout.strip())
    except ValueError as error:
        stop_process(proc, "postgres backend", log_path)
        log_file.close()
        raise EnvError(f"psql probe returned {result.stdout!r}") from error
    if rows < 1:
        stop_process(proc, "postgres backend", log_path)
        log_file.close()
        raise EnvError(
            "the binary accepted a token write but PostgreSQL has no rows: the request "
            "path is still SQLite even though PROXY_PG_BACKEND is set"
        )

def start_backend(
    *,
    binary: Path,
    backend: str,
    port: int,
    db_path: Path,
    pg_url: str | None,
    mock_port: int,
    work_dir: Path,
    health_timeout: float,
) -> tuple[subprocess.Popen[bytes], Path, Any]:
    require_free_port(port, f"{backend} backend")
    db_path.parent.mkdir(parents=True, exist_ok=True)
    log_path = work_dir / f"backend-{backend}.log"
    log_file = log_path.open("wb")
    env = {
        **os.environ,
        "TAVILY_API_KEYS": PARITY_KEY,
        "TAVILY_UPSTREAM": f"http://127.0.0.1:{mock_port}/mcp",
        "TAVILY_USAGE_BASE": f"http://127.0.0.1:{mock_port}",
        "DEV_OPEN_ADMIN": "1",
        "PROXY_BIND": "127.0.0.1",
        "PROXY_PORT": str(port),
        "PROXY_DB_PATH": str(db_path),
        "RUST_LOG": os.environ.get("RUST_LOG", "info"),
    }
    command = [
        str(binary),
        "--bind",
        "127.0.0.1",
        "--port",
        str(port),
        "--db-path",
        str(db_path),
        "--dev-open-admin",
        "--upstream",
        f"http://127.0.0.1:{mock_port}/mcp",
        "--usage-base",
        f"http://127.0.0.1:{mock_port}",
        "--keys",
        PARITY_KEY,
    ]
    if backend == "postgres":
        if not pg_url:
            raise EnvError("postgres backend requires --pg-url")
        env["PROXY_PG_BACKEND"] = "1"
        env["PROXY_DATABASE_URL"] = pg_url
        command.extend(["--database-url", pg_url])

    proc = subprocess.Popen(
        command,
        stdout=log_file,
        stderr=subprocess.STDOUT,
        env=env,
    )
    try:
        wait_for_health(port, proc, log_path, timeout=health_timeout)
    except EnvError as error:
        log_text = log_path.read_text(errors="replace")
        stop_process(proc, f"{backend} backend", log_path)
        log_file.close()
        if backend == "postgres" and "pg_runtime_not_serving" in log_text:
            raise EnvError(
                "this binary validates PostgreSQL at startup but still serves the "
                "SQLite request path (log event pg_runtime_not_serving); "
                "PROXY_PG_BACKEND is not implemented in this build"
            ) from error
        raise EnvError(f"{error}\n--- backend log tail ---\n{tail(log_path)}") from error
    if backend == "postgres":
        # The startup log always carries `pg_runtime_not_serving` while the port lands wave by
        # wave, so it cannot prove the request path. Prove it behaviourally instead: create a
        # token and require the row to exist in PostgreSQL.
        verify_postgres_serves(port, pg_url, proc, log_path, log_file)
    return proc, log_path, log_file


# ---------------------------------------------------------------------------
# Comparison
# ---------------------------------------------------------------------------


def _is_trivial_float_difference(left: float, right: float) -> bool:
    scale = max(abs(left), abs(right), 1.0)
    return abs(left - right) <= TRIVIAL_RELATIVE_DELTA * scale


def diff_values(
    left: Any,
    right: Any,
    *,
    path: str = "",
    differences: list[dict[str, Any]],
    tolerated: list[dict[str, Any]],
) -> None:
    if isinstance(left, dict) and isinstance(right, dict):
        left_keys = set(left)
        right_keys = set(right)
        if path == "":
            left_keys -= IGNORED_TOP_LEVEL
            right_keys -= IGNORED_TOP_LEVEL
        for key in sorted(left_keys - right_keys):
            differences.append(
                {"path": f"{path}.{key}" if path else key, "sqlite": left[key], "postgres": "<missing>"}
            )
        for key in sorted(right_keys - left_keys):
            differences.append(
                {"path": f"{path}.{key}" if path else key, "sqlite": "<missing>", "postgres": right[key]}
            )
        for key in sorted(left_keys & right_keys):
            child_path = f"{path}.{key}" if path else key
            if IGNORED_KEY_RE.search(key):
                if left[key] != right[key]:
                    tolerated.append(
                        {"path": child_path, "sqlite": left[key], "postgres": right[key], "reason": "identifier/timestamp"}
                    )
                continue
            diff_values(
                left[key],
                right[key],
                path=child_path,
                differences=differences,
                tolerated=tolerated,
            )
        return
    if isinstance(left, list) and isinstance(right, list):
        if len(left) != len(right):
            differences.append(
                {"path": path, "sqlite": f"{len(left)} entries", "postgres": f"{len(right)} entries"}
            )
            return
        for index, (left_item, right_item) in enumerate(zip(left, right)):
            diff_values(
                left_item,
                right_item,
                path=f"{path}[{index}]",
                differences=differences,
                tolerated=tolerated,
            )
        return
    if isinstance(left, bool) or isinstance(right, bool):
        if left is not right:
            differences.append({"path": path, "sqlite": left, "postgres": right})
        return
    if isinstance(left, (int, float)) and isinstance(right, (int, float)):
        if left == right:
            return
        if isinstance(left, float) or isinstance(right, float):
            if _is_trivial_float_difference(float(left), float(right)):
                tolerated.append(
                    {"path": path, "sqlite": left, "postgres": right, "reason": "float last digit"}
                )
                return
        differences.append({"path": path, "sqlite": left, "postgres": right})
        return
    if left != right:
        differences.append({"path": path, "sqlite": left, "postgres": right})


def compare_traffic(
    sqlite_payload: dict[str, Any], pg_payload: dict[str, Any]
) -> tuple[bool, list[dict[str, Any]]]:
    differences: list[dict[str, Any]] = []
    sqlite_requests = sqlite_payload.get("requests", [])
    pg_requests = pg_payload.get("requests", [])
    if len(sqlite_requests) != len(pg_requests):
        differences.append(
            {
                "path": "traffic.requests",
                "sqlite": f"{len(sqlite_requests)} requests",
                "postgres": f"{len(pg_requests)} requests",
            }
        )
    # Order and routing are deterministic; the outcome status of a throttled request is not,
    # because the rate-limit window advances with wall-clock time. Per-request status is
    # therefore aggregated into the summary instead of compared position by position.
    fields = ("seq", "route", "method", "rpc_method")
    for index, (left, right) in enumerate(zip(sqlite_requests, pg_requests)):
        for field in fields:
            if left.get(field) != right.get(field):
                differences.append(
                    {
                        "path": f"traffic.requests[{index}].{field}",
                        "sqlite": left.get(field),
                        "postgres": right.get(field),
                    }
                )
    if sqlite_payload.get("summary") != pg_payload.get("summary"):
        differences.append(
            {
                "path": "traffic.summary",
                "sqlite": sqlite_payload.get("summary"),
                "postgres": pg_payload.get("summary"),
            }
        )
    return not differences, differences


def compare_facts(
    sqlite_facts: dict[str, Any],
    pg_facts: dict[str, Any],
    *,
    skip_manual_quota_sync: bool,
) -> tuple[bool, list[dict[str, Any]], list[dict[str, Any]]]:
    differences: list[dict[str, Any]] = []
    tolerated: list[dict[str, Any]] = []
    left = json.loads(json.dumps(sqlite_facts))
    right = json.loads(json.dumps(pg_facts))
    if skip_manual_quota_sync:
        # The key-pool balance follows scheduler timing when the manual sync probe is
        # skipped, so it is reported as not asserted instead of compared.
        for side in (left, right):
            if isinstance(side, dict):
                side.pop("account_balance", None)
    diff_values(
        left,
        right,
        differences=differences,
        tolerated=tolerated,
    )
    return not differences, differences, tolerated


def internal_fact_errors(facts: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    auth = facts["auth_token_logs"]
    if auth["total"] != sum(auth["by_result_status"].values()):
        errors.append("auth_token_logs.total != sum(by_result_status)")
    if auth["total"] != sum(auth["by_request_kind_key"].values()):
        errors.append("auth_token_logs.total != sum(by_request_kind_key)")
    logs = facts["request_logs"]
    if logs["total"] != sum(logs["by_visibility"].values()):
        errors.append("request_logs.total != sum(by_visibility)")
    alerts = facts["alerts"]
    if alerts["total"] != sum(row["count"] for row in alerts["by_group_key_status"]):
        errors.append("alerts.total != sum(by_group_key_status)")
    if not facts["invariant"]["matched"]:
        errors.append(
            "bucket total_requests != visible request_logs with api_key_id "
            f"({facts['invariant']['bucket_total_requests']} vs "
            f"{facts['invariant']['request_logs_visible_with_key']})"
        )
    return errors


# ---------------------------------------------------------------------------
# Orchestration
# ---------------------------------------------------------------------------


def run_one_backend(
    *,
    backend: str,
    binary: Path,
    port: int,
    work_dir: Path,
    pg_url: str | None,
    mock_port: int,
    seed: int,
    duration_secs: int,
    settle_secs: float,
    persist_timeout: float,
    health_timeout: float,
    skip_manual_quota_sync: bool,
) -> dict[str, Any]:
    db_path = work_dir / backend / "core.db"
    if db_path.parent.exists():
        # Only the backend-owned subdirectory is recreated; the rest of the
        # work directory (logs from other backends) is left alone.
        shutil.rmtree(db_path.parent)
    proc, _log_path, log_file = start_backend(
        binary=binary,
        backend=backend,
        port=port,
        db_path=db_path,
        pg_url=pg_url,
        mock_port=mock_port,
        work_dir=work_dir,
        health_timeout=health_timeout,
    )
    base_url = f"http://127.0.0.1:{port}"
    try:
        raise_rate_limits(base_url)
        if skip_manual_quota_sync:
            print(f"[{backend}] skipping manual quota sync; the key-pool balance is not asserted")
        else:
            pin_key_quota(base_url)
        print(f"[{backend}] running {(duration_secs / traffic_mod.CYCLE_SECS):.0f} traffic cycles")
        traffic_payload = traffic_mod.run(
            base_url,
            seed=seed,
            duration_secs=duration_secs,
            output_path=work_dir / f"traffic-{backend}.json",
        )
        settled_total = wait_for_request_logs(base_url, persist_timeout)
        print(f"[{backend}] request logs settled at {settled_total} rows")
        if settle_secs > 0:
            time.sleep(settle_secs)
    finally:
        stop_process(proc, f"{backend} backend", _log_path)
        log_file.close()
    # Export after shutdown: pending request logs and rollup flushes are committed
    # and SQLite WAL frames are no longer racing with a live writer. Reading while
    # the process runs produced non-atomic per-table snapshots (observed: bucket
    # rebuild mid-commit, late MCP request-log writes).
    if backend == "postgres":
        if not pg_url:
            raise EnvError("postgres backend requires --pg-url")
        facts_payload = facts_mod.export_postgres(
            pg_url, work_dir / f"facts-{backend}.json"
        )
        split_rows = sidecar_request_log_rows(db_path)
        if split_rows:
            print(
                f"[postgres] {split_rows} request-log rows landed in the SQLite sidecar: the "
                "store is still partially ported"
            )
            raise EnvError(
                "this binary serves the request path from PostgreSQL but still writes "
                "request-log, billing and quota rows through SQLite lanes. Quota enforcement "
                "and accounting therefore cannot be compared across backends, and the "
                "PostgreSQL side under-enforces quota as a result. Finish the port, then run "
                "this gate again without --skip-manual-quota-sync."
            )
    else:
        facts_payload = facts_mod.export_sqlite(
            db_path, work_dir / f"facts-{backend}.json"
        )
    return {"traffic": traffic_payload, "facts": facts_payload}


def sidecar_request_log_rows(db_path: Path) -> int:
    """Count request-log rows the still-unported lanes wrote to the SQLite sidecar."""
    sidecar = db_path.with_name("core-observability.db")
    if not sidecar.is_file():
        return 0
    try:
        connection = sqlite3.connect(f"file:{sidecar}?mode=ro", uri=True)
    except sqlite3.Error:
        return 0
    try:
        row = connection.execute("SELECT COUNT(*) FROM request_logs").fetchone()
    except sqlite3.Error:
        return 0
    finally:
        connection.close()
    return int(row[0]) if row else 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Run one deterministic load against the SQLite and PostgreSQL storage "
            "backends of the same binary, then compare the stored facts."
        )
    )
    parser.add_argument("--binary", required=True, type=Path, help="binary under test")
    parser.add_argument(
        "--pg-url",
        required=True,
        help="PostgreSQL connection string for a dedicated test database",
    )
    parser.add_argument(
        "--duration-secs",
        type=int,
        default=120,
        help="traffic duration per backend (default: 120)",
    )
    parser.add_argument(
        "--work-dir",
        type=Path,
        default=None,
        help="temporary directory (default: a fresh mkdtemp directory)",
    )
    parser.add_argument(
        "--skip-sqlite",
        action="store_true",
        help="run only the PostgreSQL backend and check its internal invariants",
    )
    parser.add_argument(
        "--keep",
        action="store_true",
        help="keep the temporary directory after a successful run",
    )
    parser.add_argument("--seed", type=int, default=DEFAULT_SEED)
    parser.add_argument("--mock-upstream-port", type=int, default=DEFAULT_MOCK_PORT)
    parser.add_argument("--backend-port", type=int, default=DEFAULT_BACKEND_PORT)
    parser.add_argument(
        "--sqlite-backend-port", type=int, default=DEFAULT_SQLITE_PORT
    )
    parser.add_argument("--health-timeout-secs", type=float, default=90.0)
    parser.add_argument(
        "--settle-secs",
        type=float,
        default=5.0,
        help="wait for rollup flush before shutdown (default: 5)",
    )
    parser.add_argument(
        "--persist-timeout-secs",
        type=float,
        default=30.0,
        help="max wait for deferred request-log writes to become visible (default: 30)",
    )
    parser.add_argument(
        "--no-reset-pg",
        action="store_true",
        help="do not drop core/observability schemas before the PostgreSQL run",
    )
    parser.add_argument(
        "--skip-manual-quota-sync",
        action="store_true",
        help=(
            "skip the manual quota sync probe; a partially ported store cannot write the "
            "quota sync job through both backends, and the key-pool balance fact is then "
            "reported as not asserted"
        ),
    )
    parser.add_argument(
        "--allow-non-local-pg",
        action="store_true",
        help="allow a globally routable PostgreSQL host (dedicated test database only)",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)

    binary = args.binary.expanduser().resolve()
    if not binary.is_file():
        print(f"error: --binary does not exist: {binary}", file=sys.stderr)
        return 2
    if not os.access(binary, os.X_OK):
        print(f"error: --binary is not executable: {binary}", file=sys.stderr)
        return 2
    if args.duration_secs <= 0:
        print("error: --duration-secs must be positive", file=sys.stderr)
        return 2
    if shutil.which("psql") is None:
        print("error: psql is required to export PostgreSQL facts", file=sys.stderr)
        return 2

    validate_pg_url(args.pg_url, args.allow_non_local_pg)

    work_dir: Path
    if args.work_dir is None:
        work_dir = Path(tempfile.mkdtemp(prefix="pg-parity-"))
    else:
        work_dir = args.work_dir.expanduser().resolve()
        work_dir.mkdir(parents=True, exist_ok=True)

    keep = args.keep
    exit_code = 2
    mock_proc: subprocess.Popen[bytes] | None = None
    mock_log_file: Any = None
    try:
        require_free_port(args.mock_upstream_port, "mock upstream")
        require_free_port(args.backend_port, "postgres backend")
        if not args.skip_sqlite:
            require_free_port(args.sqlite_backend_port, "sqlite backend")

        print(f"work dir: {work_dir}")
        print(f"postgres target: {redact_url(args.pg_url)}")
        mock_proc, _mock_log_path, mock_log_file = start_mock_upstream(
            work_dir, args.mock_upstream_port
        )
        print(f"mock upstream: http://127.0.0.1:{args.mock_upstream_port}")

        if not args.no_reset_pg:
            print("resetting core/observability schemas in the test database")
            reset_pg_schemas(args.pg_url)

        results: dict[str, Any] = {}
        results["postgres"] = run_one_backend(
            backend="postgres",
            binary=binary,
            port=args.backend_port,
            work_dir=work_dir,
            pg_url=args.pg_url,
            mock_port=args.mock_upstream_port,
            seed=args.seed,
            duration_secs=args.duration_secs,
            settle_secs=args.settle_secs,
            persist_timeout=args.persist_timeout_secs,
            health_timeout=args.health_timeout_secs,
            skip_manual_quota_sync=args.skip_manual_quota_sync,
        )
        if args.skip_sqlite:
            facts_payload = results["postgres"]["facts"]
            errors = internal_fact_errors(facts_payload)
            report = {
                "ok": not errors,
                "exit_code": 0 if not errors else 1,
                "mode": "postgres-only",
                "seed": args.seed,
                "duration_secs": args.duration_secs,
                "work_dir": str(work_dir),
                "internal_errors": errors,
            }
            (work_dir / "report.json").write_text(
                json.dumps(report, indent=2, sort_keys=True) + "\n"
            )
            print(json.dumps(report, indent=2, sort_keys=True))
            exit_code = 0 if not errors else 1
            return exit_code

        results["sqlite"] = run_one_backend(
            backend="sqlite",
            binary=binary,
            port=args.sqlite_backend_port,
            work_dir=work_dir,
            pg_url=None,
            mock_port=args.mock_upstream_port,
            seed=args.seed,
            duration_secs=args.duration_secs,
            settle_secs=args.settle_secs,
            persist_timeout=args.persist_timeout_secs,
            health_timeout=args.health_timeout_secs,
            skip_manual_quota_sync=args.skip_manual_quota_sync,
        )

        traffic_ok, traffic_differences = compare_traffic(
            results["sqlite"]["traffic"], results["postgres"]["traffic"]
        )
        facts_ok, facts_differences, tolerated = compare_facts(
            results["sqlite"]["facts"],
            results["postgres"]["facts"],
            skip_manual_quota_sync=args.skip_manual_quota_sync,
        )
        internal_errors = [
            f"sqlite: {error}"
            for error in internal_fact_errors(results["sqlite"]["facts"])
        ] + [
            f"postgres: {error}"
            for error in internal_fact_errors(results["postgres"]["facts"])
        ]
        ok = traffic_ok and facts_ok and not internal_errors
        exit_code = 0 if ok else 1
        report = {
            "ok": ok,
            "exit_code": exit_code,
            "seed": args.seed,
            "duration_secs": args.duration_secs,
            "work_dir": str(work_dir),
            "traffic": {
                "identical": traffic_ok,
                "differences": traffic_differences,
            },
            "facts": {
                "identical": facts_ok,
                "differences": facts_differences,
                "tolerated": tolerated,
            },
            "internal_errors": internal_errors,
        }
        (work_dir / "report.json").write_text(
            json.dumps(report, indent=2, sort_keys=True) + "\n"
        )
        _print_summary(report)
        return exit_code
    except (EnvError, traffic_mod.TrafficError, facts_mod.FactsError) as error:
        print(f"environment error: {error}", file=sys.stderr)
        if mock_log_file is not None:
            print(f"mock upstream log: {work_dir / 'mock-upstream.log'}", file=sys.stderr)
        keep = True
        exit_code = 2
        return exit_code
    except OSError as error:
        print(f"environment error: {error}", file=sys.stderr)
        keep = True
        exit_code = 2
        return exit_code
    finally:
        if mock_proc is not None:
            stop_process(mock_proc, "mock upstream", work_dir / "mock-upstream.log")
        if mock_log_file is not None:
            mock_log_file.close()
        if not keep:
            shutil.rmtree(work_dir, ignore_errors=True)
        else:
            print(f"artifacts kept in {work_dir}", file=sys.stderr)


def _print_summary(report: dict[str, Any]) -> None:
    print()
    print(f"traffic identical: {report['traffic']['identical']}")
    print(f"facts identical:   {report['facts']['identical']}")
    if report["traffic"]["differences"]:
        print("traffic differences:")
        for item in report["traffic"]["differences"][:50]:
            print(f"  {item['path']}: sqlite={item['sqlite']!r} postgres={item['postgres']!r}")
    if report["facts"]["differences"]:
        print("fact differences:")
        for item in report["facts"]["differences"][:50]:
            print(f"  {item['path']}: sqlite={item['sqlite']!r} postgres={item['postgres']!r}")
    if report["facts"]["tolerated"]:
        print("tolerated differences:")
        for item in report["facts"]["tolerated"][:50]:
            print(
                f"  {item['path']}: sqlite={item['sqlite']!r} postgres={item['postgres']!r} "
                f"({item.get('reason') or item.get('note') or 'tolerated'})"
            )
    if report["internal_errors"]:
        print("internal invariant errors:")
        for error in report["internal_errors"]:
            print(f"  {error}")
    print(f"exit code: {report['exit_code']} (0 identical, 1 different, 2 environment)")


if __name__ == "__main__":
    sys.exit(main())
