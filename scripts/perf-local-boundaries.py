#!/usr/bin/env python3
"""Run paired local request-cost measurements against a mock-only upstream.

The harness intentionally does not build, deploy, or contact a production service. A caller
supplies a service command template for each archived source revision. The command receives the
temporary fixture paths and fixed workload settings through PERF_LOCAL_* environment variables.
"""

from __future__ import annotations

import argparse
from contextlib import closing
import hashlib
import http.client
import json
import os
import re
import resource as posix_resource
import shlex
import signal
import shutil
import socket
import sqlite3
import statistics
import subprocess
import sys
import tarfile
import tempfile
import threading
import time
import traceback
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any, Callable, Iterable


DEFAULT_BASELINE_REF = "b1bb1043823b9260b18389854e6b90b2cdedbd70"
DEFAULT_WARMUP_SECONDS = 60
DEFAULT_DURATION_SECONDS = 600
DEFAULT_ROUNDS = 3
DEFAULT_EXTENSION_SECONDS = 1800
DEFAULT_MOCK_DELAY_MS = 200
DEFAULT_MIN_SAMPLES = 30
DEFAULT_REQUEST_INTERVAL_SECONDS = 0.25
DEFAULT_CONTROL_INTERVAL_SECONDS = 1.0
TOKEN_BOOTSTRAP_RETRY_BUDGET_SECONDS = 5.0
TOKEN_BOOTSTRAP_MAX_RETRIES_PER_LABEL = 3
BOUND_TOKEN_BOOTSTRAP_RETRY_DELAY_SECONDS = 0.25
BOUND_FIXTURE_SOURCE = (
    "SQLite synthetic users + account_quota_limits seed; management API token binding"
)
BOUND_FIXTURE_ACCOUNT_PREFIX = "perf-account-"
BOUND_FIXTURE_QUOTA_HEADROOM = {
    "business_calls_1h_limit": 100000,
    "daily_credits_limit": 100000,
    "monthly_credits_limit": 1000000,
}
DEFAULT_SUBJECTS = tuple(f"subject-{index}" for index in range(1, 15))
DEFAULT_TOKENS = tuple(f"token-{index}" for index in range(1, 15))
DEFAULT_RESOURCES = "cpu=2,memory=1GiB,sqlite_pool=3"
MAX_SERVICE_LOG_BYTES = 64 * 1024
LOG_TRUNCATION_MARKER = b"\n<LOG_TRUNCATED_HEAD_TAIL>\n"
MAX_LOG_REDACTION_CARRY = MAX_SERVICE_LOG_BYTES
LOG_REDACTION_GAP_MARKER = b"\n<LOG_REDACTION_GAP>\n"
MAX_LOG_REDACTION_GAPS = 64
MAX_EXCEPTION_TRACEBACK_BYTES = 16 * 1024
MAX_RESPONSE_EVIDENCE = 12
MAX_METRICS_SCRAPES = 256
MAX_METRICS_SCRAPE_ERRORS = 64
MIN_METRICS_SCRAPE_INTERVAL_SECONDS = 1.0
MAX_METRICS_SCRAPE_INTERVAL_SECONDS = 60.0
DEFAULT_METRICS_SCRAPE_INTERVAL_SECONDS = MIN_METRICS_SCRAPE_INTERVAL_SECONDS
DEFAULT_SYSTEM_DIAGNOSTICS_INTERVAL_SECONDS = 1.0
MAX_SYSTEM_DIAGNOSTIC_SAMPLES = 4096
MAX_SYSTEM_DIAGNOSTIC_DEVICES = 64
MAX_SERVICE_PROCESS_DESCENDANTS = 128
MAX_SERVICE_PROCESS_CANDIDATES = MAX_SERVICE_PROCESS_DESCENDANTS + 1
PROCESS_CLEANUP_TERM_TIMEOUT_SECONDS = 10.0
PROCESS_CLEANUP_KILL_TIMEOUT_SECONDS = 5.0
PHASE_SCHEMA_LEGACY = "legacy"
PHASE_SCHEMA_OPERATION = "operation"
PHASE_SCHEMA_UNSUPPORTED = "unsupported"
PHASE_METRIC_NAMES = {
    PHASE_SCHEMA_LEGACY: "hikari_http_duration_seconds",
    PHASE_SCHEMA_OPERATION: "hikari_http_request_duration_seconds",
}
SYNTHETIC_FIXTURE_SETTINGS = {
    "TOKEN_HOURLY_REQUEST_LIMIT": "100000",
    "TOKEN_HOURLY_LIMIT": "100000",
    "TOKEN_DAILY_LIMIT": "100000",
    "TOKEN_MONTHLY_LIMIT": "1000000",
    "REQUEST_RATE_LIMIT": "100000",
    "REQUEST_RATE_LIMIT_WINDOW_MINUTES": "5",
    "MCP_SESSION_AFFINITY_KEY_COUNT": "5",
    "purpose": "mock_only_business_success_budget",
    "billing_checks": "enabled",
}
SYNTHETIC_FIXTURE_ENV = {
    key: value for key, value in SYNTHETIC_FIXTURE_SETTINGS.items() if key.startswith("TOKEN_")
}
REQUIRED_SCENARIO_NAMES = (
    "serial",
    "cross_account_concurrency_4",
    "cross_account_concurrency_8",
    "same_account_queue",
)
PHASE_METRIC_SELECTOR = {
    "method": "POST",
    "operation": "search",
    "outcome": "success",
    "route_family": "http_search",
}
LEGACY_PHASE_METRIC_SELECTOR = {
    key: value for key, value in PHASE_METRIC_SELECTOR.items() if key != "operation"
}

# Keep each scenario's account identity isolated while preserving same-account
# sharing inside the queue scenario. Indices are part of the persisted contract.
SCENARIO_IDENTITY_INDICES = {
    "serial": {"credentials": (0,), "subjects": (0,)},
    "cross_account_concurrency_4": {
        "credentials": (1, 2, 3, 4),
        "subjects": (1, 2, 3, 4),
    },
    "cross_account_concurrency_8": {
        "credentials": (5, 6, 7, 8, 9, 10, 11, 12),
        "subjects": (5, 6, 7, 8, 9, 10, 11, 12),
    },
    "same_account_queue": {"credentials": (13,) * 8, "subjects": (13,) * 8},
}
REQUIRED_IDENTITY_COUNT = 14


@dataclass(frozen=True)
class Credential:
    """A bootstrapped credential with secret-free identity evidence."""

    label: str
    secret: str
    token_id: str | None
    billing_subject: str | None


def scenario_contract(interval_seconds: float) -> list[dict[str, Any]]:
    scheduling = {
        "dispatch": "fixed_rate",
        "interval_seconds": interval_seconds,
        "measurement_window": "shared",
        "start_synchronization": "barrier",
    }
    return [
        {
            "name": "serial",
            "driver": "serial",
            "concurrency": 1,
            "credential_mode": "single_token_subject",
            "credential_slots": 1,
            "expected_outcome": "success",
            "expected_status_class": "2xx",
            "credential_indices": list(SCENARIO_IDENTITY_INDICES["serial"]["credentials"]),
            "subject_indices": list(SCENARIO_IDENTITY_INDICES["serial"]["subjects"]),
            "scheduling": scheduling,
        },
        {
            "name": "cross_account_concurrency_4",
            "driver": "parallel_workers",
            "concurrency": 4,
            "credential_mode": "one_unique_token_subject_per_worker",
            "credential_slots": 4,
            "expected_outcome": "success",
            "expected_status_class": "2xx",
            "credential_indices": list(
                SCENARIO_IDENTITY_INDICES["cross_account_concurrency_4"]["credentials"]
            ),
            "subject_indices": list(
                SCENARIO_IDENTITY_INDICES["cross_account_concurrency_4"]["subjects"]
            ),
            "scheduling": scheduling,
        },
        {
            "name": "cross_account_concurrency_8",
            "driver": "parallel_workers",
            "concurrency": 8,
            "credential_mode": "one_unique_token_subject_per_worker",
            "credential_slots": 8,
            "expected_outcome": "success",
            "expected_status_class": "2xx",
            "credential_indices": list(
                SCENARIO_IDENTITY_INDICES["cross_account_concurrency_8"]["credentials"]
            ),
            "subject_indices": list(
                SCENARIO_IDENTITY_INDICES["cross_account_concurrency_8"]["subjects"]
            ),
            "scheduling": scheduling,
        },
        {
            "name": "same_account_queue",
            "driver": "parallel_workers_shared_subject",
            "concurrency": 8,
            "credential_mode": "same_token_subject_for_all_workers",
            "credential_slots": 1,
            "expected_outcome": "success",
            "expected_status_class": "2xx",
            "credential_indices": list(
                SCENARIO_IDENTITY_INDICES["same_account_queue"]["credentials"]
            ),
            "subject_indices": list(
                SCENARIO_IDENTITY_INDICES["same_account_queue"]["subjects"]
            ),
            "scheduling": scheduling,
        },
    ]


def logical_cohort(subjects: tuple[str, ...], tokens: tuple[str, ...], interval: float) -> dict[str, Any]:
    return {
        "subjects": list(subjects),
        "tokens": list(tokens),
        "scenarios": scenario_contract(interval),
        "cohort_path": "variants[*].scenarios[*].request",
        "success_cohort_path": "variants[*].scenarios[*].actual.success_request",
    }


def scenario_digest(
    credentials: tuple[Credential, ...],
    interval_seconds: float = DEFAULT_REQUEST_INTERVAL_SECONDS,
) -> str:
    payload = {
        "scenario_contract": scenario_contract(interval_seconds),
        "credentials": [
            {
                "label": credential.label,
                "token_id": credential.token_id,
                "billing_subject": credential.billing_subject,
            }
            for credential in credentials
        ],
    }
    canonical = json.dumps(payload, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def credential_mapping_evidence(
    credentials: tuple[Credential, ...],
    interval_seconds: float = DEFAULT_REQUEST_INTERVAL_SECONDS,
) -> dict[str, Any]:
    verified = bool(credentials) and all(
        credential.token_id and credential.billing_subject for credential in credentials
    )
    bound_account = verified and all(
        credential.billing_subject.startswith("account:") for credential in credentials
    )
    evidence: dict[str, Any] = {
        "status": "verified" if verified else "synthetic_labels_only",
        "source": BOUND_FIXTURE_SOURCE if bound_account else "POST /api/tokens response token id + billing_subject_for_token",
        "credential_kind": "bound_access_token" if bound_account else "unbound_access_token",
        "billing_subject_kind": "account" if bound_account else "token",
        "binding_path": (
            "POST /api/users/<account-id>/tokens"
            if bound_account
            else "POST /api/tokens"
        ),
        "secret_persisted": False,
        "scenario_digest": scenario_digest(credentials, interval_seconds),
        "credentials": [
            {
                "label": credential.label,
                "token_id": credential.token_id,
                "billing_subject": credential.billing_subject,
            }
            for credential in credentials
        ],
        "same_account_definition": "same token_id and billing_subject reused by every queue worker",
        "cross_account_definition": "one distinct token_id and billing_subject per worker",
    }
    if bound_account:
        evidence["account_ids"] = [
            credential.billing_subject.removeprefix("account:")
            for credential in credentials
        ]
        evidence["quota_headroom"] = dict(BOUND_FIXTURE_QUOTA_HEADROOM)
    return evidence


def credential_mapping_metadata(args: argparse.Namespace) -> dict[str, Any]:
    bound_account = bool(getattr(args, "bootstrap_bound_business_tokens", False))
    tokens = tuple(getattr(args, "tokens", ()))
    mapping: dict[str, Any] = {
        "status": "unvalidated",
        "source": (
            BOUND_FIXTURE_SOURCE
            if bound_account
            else "POST /api/tokens response token id + billing_subject_for_token"
        ),
        "credential_kind": (
            "bound_access_token" if bound_account else "unbound_access_token"
        ),
        "billing_subject_kind": "account" if bound_account else "token",
        "binding_path": (
            "POST /api/users/<account-id>/tokens"
            if bound_account
            else "POST /api/tokens"
        ),
        "secret_persisted": False,
        "same_account_definition": "same token_id and billing_subject reused by every queue worker",
        "cross_account_definition": "one distinct token_id and billing_subject per worker",
    }
    if bound_account:
        mapping["account_ids"] = list(bound_fixture_account_ids(tokens))
        mapping["expected_credential_count"] = len(tokens)
        mapping["binding_kind"] = "bound_account"
    return mapping


class MockUpstreamHandler(BaseHTTPRequestHandler):
    delay_seconds = DEFAULT_MOCK_DELAY_MS / 1000
    request_count = 0
    response_status_counts: dict[str, int] = {}
    observation_lock = threading.Lock()

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
        with self.observation_lock:
            type(self).request_count += 1
            key = "200"
            type(self).response_status_counts[key] = (
                type(self).response_status_counts.get(key, 0) + 1
            )
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
        with MockUpstreamHandler.observation_lock:
            MockUpstreamHandler.request_count = 0
            MockUpstreamHandler.response_status_counts = {}
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    @property
    def url(self) -> str:
        return f"http://127.0.0.1:{self.server.server_port}"

    def evidence(self) -> dict[str, Any]:
        with MockUpstreamHandler.observation_lock:
            return {
                "status": "ready",
                "url_kind": "loopback_synthetic_mock",
                "request_count": MockUpstreamHandler.request_count,
                "response_statuses": dict(MockUpstreamHandler.response_status_counts),
                "response_usage_credits": 1,
                "response_contract": "results + usage.credits=1",
            }

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


def _decode_mountinfo_field(value: str) -> str:
    return (
        value.replace(r"\040", " ")
        .replace(r"\011", "\t")
        .replace(r"\012", "\n")
        .replace(r"\134", "\\")
    )


def mount_observation(path: Path) -> dict[str, Any]:
    requested = path.resolve()
    probe = requested
    while not probe.exists() and probe != probe.parent:
        probe = probe.parent
    evidence: dict[str, Any] = {
        "status": "unavailable",
        "path": str(requested),
        "probe_path": str(probe),
    }
    try:
        evidence["device"] = os.stat(probe).st_dev
    except OSError as error:
        evidence["error"] = {
            "type": type(error).__name__,
            "errno": getattr(error, "errno", None),
            "message": redact_output(str(error)),
        }
        return evidence

    mountinfo_path = Path("/proc/self/mountinfo")
    if not mountinfo_path.is_file():
        evidence["reason"] = "mountinfo_unavailable"
        return evidence
    best: dict[str, Any] | None = None
    try:
        lines = mountinfo_path.read_text(encoding="utf-8").splitlines()
    except OSError as error:
        evidence["reason"] = f"mountinfo_read:{type(error).__name__}"
        return evidence
    for line in lines:
        fields = line.split()
        if len(fields) < 10 or "-" not in fields:
            continue
        mount_point = Path(_decode_mountinfo_field(fields[4]))
        if not (probe == mount_point or mount_point in probe.parents):
            continue
        separator = fields.index("-")
        if len(fields) <= separator + 3:
            continue
        candidate = {
            "mount_point": str(mount_point),
            "filesystem_type": fields[separator + 1],
            "source": _decode_mountinfo_field(fields[separator + 2]),
            "super_options": fields[separator + 3],
        }
        if best is None or len(mount_point.parts) > len(Path(best["mount_point"]).parts):
            best = candidate
    if best is None:
        evidence["reason"] = "mount_not_found"
        return evidence
    evidence.update({"status": "observed", **best})
    return evidence


def _filesystem_storage_observation(path: Path) -> dict[str, Any]:
    requested = path.resolve()
    probe = requested
    while not probe.exists() and probe != probe.parent:
        probe = probe.parent
    evidence: dict[str, Any] = {
        "path": str(requested),
        "probe_path": str(probe),
        "exists": requested.exists(),
        "mount": mount_observation(probe),
    }
    try:
        stat = os.statvfs(probe)
    except OSError as error:
        evidence["status"] = "unavailable"
        evidence["error"] = {
            "type": type(error).__name__,
            "errno": getattr(error, "errno", None),
            "strerror": getattr(error, "strerror", None),
            "message": redact_output(str(error)),
        }
        return evidence
    evidence.update(
        {
            "status": "observed",
            "available_bytes": stat.f_bavail * stat.f_frsize,
            "free_bytes": stat.f_bfree * stat.f_frsize,
            "total_bytes": stat.f_blocks * stat.f_frsize,
            "available_inodes": stat.f_favail,
            "free_inodes": stat.f_ffree,
            "total_inodes": stat.f_files,
        }
    )
    return evidence


def temp_directory_evidence() -> dict[str, Any]:
    path = Path(tempfile.gettempdir()).resolve()
    evidence = _filesystem_storage_observation(path)
    evidence["path"] = str(path)
    return evidence


def workspace_storage_evidence(path: Path) -> dict[str, Any]:
    evidence = _filesystem_storage_observation(path)
    evidence["tempdir"] = str(Path(tempfile.gettempdir()).resolve())
    evidence["tempdir_storage"] = temp_directory_evidence()
    return evidence


def workspace_directory_evidence(path: Path) -> dict[str, Any]:
    requested = path.resolve()
    evidence: dict[str, Any] = {
        "path": str(requested),
        "exists": requested.exists(),
        "files": 0,
        "directories": 0,
        "bytes": 0,
    }
    if not requested.exists():
        return evidence
    try:
        for current, directories, files in os.walk(requested):
            evidence["directories"] += len(directories)
            for name in files:
                evidence["files"] += 1
                try:
                    evidence["bytes"] += (Path(current) / name).stat().st_size
                except OSError:
                    pass
    except OSError as error:
        evidence["error"] = {
            "type": type(error).__name__,
            "errno": getattr(error, "errno", None),
            "message": redact_output(str(error)),
        }
    return evidence


def variant_workspace_path(
    run_directory: Path, phase: str, round_number: int, variant: str
) -> Path:
    return (
        run_directory.resolve()
        / ".work"
        / phase
        / f"round-{round_number}"
        / variant
    )


def release_variant_workspace(
    path: Path, *, cleanup_validated: bool
) -> dict[str, Any]:
    before = workspace_directory_evidence(path)
    if not cleanup_validated:
        return {
            "status": "preserved",
            "reason": "cleanup_unvalidated",
            "before": before,
            "after": workspace_directory_evidence(path),
        }
    try:
        if path.exists():
            shutil.rmtree(path)
    except OSError as error:
        return {
            "status": "unvalidated",
            "reason": "workspace_remove_failed",
            "error": {
                "type": type(error).__name__,
                "errno": getattr(error, "errno", None),
                "strerror": getattr(error, "strerror", None),
                "message": redact_output(str(error)),
            },
            "before": before,
            "after": workspace_directory_evidence(path),
        }
    return {
        "status": "released",
        "before": before,
        "after": workspace_directory_evidence(path),
    }


def exception_evidence(error: BaseException, stage: str | None = None) -> dict[str, Any]:
    traceback_text = redact_output(
        "".join(traceback.format_exception(type(error), error, error.__traceback__))
    )
    if len(traceback_text) > MAX_EXCEPTION_TRACEBACK_BYTES:
        traceback_text = (
            traceback_text[:MAX_EXCEPTION_TRACEBACK_BYTES]
            + "\n<TRACEBACK_TRUNCATED>\n"
        )
    evidence: dict[str, Any] = {
        "type": type(error).__name__,
        "errno": getattr(error, "errno", None),
        "strerror": redact_output(str(getattr(error, "strerror", "") or "")),
        "filename": redact_output(str(getattr(error, "filename", "") or "")),
        "filename2": redact_output(str(getattr(error, "filename2", "") or "")),
        "message": redact_output(str(error)),
        "traceback": traceback_text,
    }
    if stage is not None:
        evidence["stage"] = stage
    return evidence


class VariantPreparationError(RuntimeError):
    def __init__(
        self, stage: str, error: BaseException, context: dict[str, Any]
    ) -> None:
        super().__init__(str(error))
        self.stage = stage
        self.error = error
        self.context = context


def close_bounded_logs(
    stdout_log: BoundedLogWriter | None,
    stderr_log: BoundedLogWriter | None,
) -> dict[str, Any]:
    errors: list[dict[str, Any]] = []
    for name, writer in (("stdout", stdout_log), ("stderr", stderr_log)):
        if writer is None or writer.evidence().get("closed"):
            continue
        try:
            writer.close()
        except BaseException as error:
            errors.append(exception_evidence(error, f"{name}_log.close"))
    closed = all(
        writer is None or writer.evidence().get("closed")
        for writer in (stdout_log, stderr_log)
    )
    return {
        "status": "validated" if closed and not errors else "unvalidated",
        "closed": closed,
        "errors": errors,
    }


class BoundedLogWriter:
    def __init__(self, path: Path, max_bytes: int = MAX_SERVICE_LOG_BYTES) -> None:
        if max_bytes < 1:
            raise ValueError("max_bytes must be positive")
        self.path = path
        self.max_bytes = max_bytes
        self._marker = LOG_TRUNCATION_MARKER[:max_bytes]
        self._capacity = max(0, max_bytes - len(self._marker))
        self._head_capacity = (self._capacity + 1) // 2
        self._tail_capacity = self._capacity - self._head_capacity
        self._head = bytearray()
        self._tail = bytearray()
        self._input_bytes = 0
        self._redacted_bytes = 0
        self._record = bytearray()
        self._record_start_byte: int | None = None
        self._dropping_record = False
        self._redaction_gap_start: int | None = None
        self._redaction_gap_bytes = 0
        self._redaction_gap_count = 0
        self._redaction_gaps: list[dict[str, int | str]] = []
        self._redaction_gap_overflow_bytes = 0
        self._closed = False
        self._handle = path.open("wb")

    def _retain(self, encoded: bytes) -> None:
        if not encoded:
            return
        self._redacted_bytes += len(encoded)
        head_remaining = self._head_capacity - len(self._head)
        if head_remaining > 0:
            self._head.extend(encoded[:head_remaining])
        tail = encoded[head_remaining:]
        if self._tail_capacity > 0 and tail:
            self._tail.extend(tail)
            if len(self._tail) > self._tail_capacity:
                del self._tail[: len(self._tail) - self._tail_capacity]

    def _begin_redaction_gap(self, start_byte: int) -> None:
        if not self._dropping_record:
            self._dropping_record = True
            self._redaction_gap_start = start_byte
            self._redaction_gap_bytes = 0

    def _add_redaction_gap_bytes(self, count: int) -> None:
        self._redaction_gap_bytes += count

    def _finish_redaction_gap(self, end_byte: int) -> None:
        if not self._dropping_record:
            return
        start_byte = self._redaction_gap_start
        if start_byte is None:
            start_byte = max(0, end_byte - self._redaction_gap_bytes)
        gap = {
            "kind": "oversized_record",
            "start_byte": start_byte,
            "end_byte": end_byte,
            "bytes": self._redaction_gap_bytes,
        }
        self._redaction_gap_count += 1
        if len(self._redaction_gaps) < MAX_LOG_REDACTION_GAPS:
            self._redaction_gaps.append(gap)
        else:
            self._redaction_gap_overflow_bytes += self._redaction_gap_bytes
        self._retain(LOG_REDACTION_GAP_MARKER)
        self._dropping_record = False
        self._redaction_gap_start = None
        self._redaction_gap_bytes = 0

    def _process_record(self, record: bytes) -> None:
        self._retain(redact_output(record.decode("utf-8", errors="replace")).encode("utf-8"))

    def _consume_fragment(self, fragment: bytes, start_byte: int) -> None:
        if not fragment:
            return
        if self._dropping_record:
            self._add_redaction_gap_bytes(len(fragment))
            return
        record_size = len(self._record) + len(fragment)
        if record_size > MAX_LOG_REDACTION_CARRY:
            self._begin_redaction_gap(
                self._record_start_byte
                if self._record_start_byte is not None
                else start_byte
            )
            self._add_redaction_gap_bytes(record_size)
            self._record.clear()
            self._record_start_byte = None
            return
        if not self._record:
            self._record_start_byte = start_byte
        self._record.extend(fragment)

    def _consume_complete_record(
        self, record: bytes, start_byte: int, end_byte: int
    ) -> None:
        if self._dropping_record:
            self._add_redaction_gap_bytes(len(record))
            self._finish_redaction_gap(end_byte)
            return
        record_size = len(self._record) + len(record)
        if record_size > MAX_LOG_REDACTION_CARRY:
            self._begin_redaction_gap(
                self._record_start_byte
                if self._record_start_byte is not None
                else start_byte
            )
            self._add_redaction_gap_bytes(record_size)
            self._record.clear()
            self._record_start_byte = None
            self._finish_redaction_gap(end_byte)
            return
        if self._record:
            combined = bytes(self._record) + record
            self._record.clear()
            self._record_start_byte = None
            self._process_record(combined)
        else:
            self._process_record(record)

    def write(self, chunk: bytes) -> None:
        if self._closed:
            raise ValueError("cannot write to a closed log")
        if not chunk:
            return
        chunk_start = self._input_bytes
        self._input_bytes += len(chunk)
        position = 0
        while position < len(chunk):
            newline = chunk.find(b"\n", position)
            if newline < 0:
                self._consume_fragment(chunk[position:], chunk_start + position)
                return
            end = newline + 1
            self._consume_complete_record(
                chunk[position:end], chunk_start + position, chunk_start + end
            )
            position = end

    def close(self) -> None:
        if self._closed:
            return
        if self._dropping_record:
            self._finish_redaction_gap(self._input_bytes)
        elif self._record:
            self._process_record(bytes(self._record))
            self._record.clear()
            self._record_start_byte = None
        retained_bytes = len(self._head) + len(self._tail)
        truncated = self._redacted_bytes > retained_bytes
        payload = bytes(self._head)
        if truncated:
            payload += self._marker
        payload += bytes(self._tail)
        self._handle.write(payload[: self.max_bytes])
        self._handle.flush()
        os.fsync(self._handle.fileno())
        self._handle.close()
        self._closed = True

    def evidence(self) -> dict[str, Any]:
        retained_bytes = len(self._head) + len(self._tail)
        truncated = self._redacted_bytes > retained_bytes
        ranges: list[dict[str, int | str]] = []
        if self._head:
            ranges.append({"kind": "head", "start_byte": 0, "end_byte": len(self._head)})
        if self._tail:
            tail_start = max(0, self._redacted_bytes - len(self._tail))
            ranges.append(
                {
                    "kind": "tail",
                    "start_byte": tail_start,
                    "end_byte": self._redacted_bytes,
                }
            )
        dropped_bytes = max(0, self._redacted_bytes - retained_bytes)
        dropped_ranges: list[dict[str, int | str]] = []
        if dropped_bytes:
            dropped_ranges.append(
                {
                    "kind": "dropped",
                    "start_byte": len(self._head),
                    "end_byte": max(len(self._head), self._redacted_bytes - len(self._tail)),
                }
            )
        return {
            "path": str(self.path),
            "max_bytes": self.max_bytes,
            "strategy": "head_tail",
            "input_bytes": self._input_bytes,
            "redacted_bytes": self._redacted_bytes,
            "retained_bytes": retained_bytes,
            "dropped_bytes": dropped_bytes,
            "truncated": truncated,
            "retained_ranges": ranges,
            "dropped_ranges": dropped_ranges,
            "redaction_gap_bytes": sum(
                int(gap["bytes"]) for gap in self._redaction_gaps
            ) + self._redaction_gap_overflow_bytes,
            "redaction_gap_count": self._redaction_gap_count,
            "redaction_gaps": self._redaction_gaps,
            "redaction_gap_overflow_bytes": self._redaction_gap_overflow_bytes,
            "closed": self._closed,
        }


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _manifest_value(manifest: dict[str, Any], build: dict[str, Any], key: str) -> Any:
    if key in build:
        return build[key]
    return manifest.get(key)


def binary_manifest_evidence(
    manifest_template: str | None,
    repo: Path,
    variant: str,
    resolved_revision: str,
) -> dict[str, Any]:
    evidence: dict[str, Any] = {
        "status": "missing" if not manifest_template else "invalid",
        "reason": "binary_manifest_not_supplied" if not manifest_template else None,
        "manifest_path": None,
        "source_sha": None,
        "build_input_sha": None,
        "binary_path": None,
        "binary_sha256": None,
        "compiler": None,
        "profile": None,
        "flags": None,
        "ci": {},
    }
    if not manifest_template:
        return evidence
    try:
        rendered = manifest_template.format(
            repo=str(repo),
            variant=variant,
            revision=resolved_revision,
        )
        manifest_path = Path(rendered).expanduser().resolve()
    except (KeyError, ValueError) as error:
        evidence["reason"] = f"binary_manifest_template_invalid:{type(error).__name__}"
        return evidence
    evidence["manifest_path"] = str(manifest_path)
    if not manifest_path.is_file():
        evidence["reason"] = "binary_manifest_missing"
        return evidence
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        evidence["reason"] = f"binary_manifest_unreadable:{type(error).__name__}"
        return evidence
    if not isinstance(manifest, dict):
        evidence["reason"] = "binary_manifest_not_object"
        return evidence

    build = manifest.get("build")
    if not isinstance(build, dict):
        build = {}
    binary = manifest.get("binary")
    if not isinstance(binary, dict):
        binary = {}
    source_sha = (
        manifest.get("source_sha")
        or manifest.get("source_revision")
        or build.get("source_sha")
    )
    ci = manifest.get("ci")
    if not isinstance(ci, dict):
        ci = {}
    run_head_sha = manifest.get("run_head_sha") or ci.get("run_head_sha")
    checkout_sha = manifest.get("checkout_sha") or ci.get("checkout_sha")
    build_input_sha = manifest.get("build_input_sha") or build.get("input_sha") or checkout_sha
    if source_sha is not None:
        evidence["source_sha"] = source_sha
    if build_input_sha is not None:
        evidence["build_input_sha"] = build_input_sha
    evidence["ci"] = {
        key: value
        for key, value in {
            "run_id": manifest.get("run_id") or ci.get("run_id"),
            "run_head_sha": run_head_sha,
            "checkout_sha": checkout_sha,
            "artifact_id": manifest.get("artifact_id") or ci.get("artifact_id"),
            "artifact_name": manifest.get("artifact_name") or ci.get("artifact_name"),
        }.items()
        if value is not None
    }
    identity_values = [source_sha, build_input_sha]
    if resolved_revision not in identity_values:
        evidence["reason"] = "binary_manifest_source_mismatch"
        return evidence

    binary_path_value = binary.get("path") or manifest.get("binary_path")
    expected_hash = binary.get("sha256") or manifest.get("binary_sha256")
    compiler = _manifest_value(manifest, build, "compiler")
    profile = _manifest_value(manifest, build, "profile")
    flags = _manifest_value(manifest, build, "flags")
    if not isinstance(binary_path_value, str) or not binary_path_value:
        evidence["reason"] = "binary_manifest_binary_path_missing"
        return evidence
    binary_path = Path(binary_path_value).expanduser()
    if not binary_path.is_absolute():
        binary_path = manifest_path.parent / binary_path
    binary_path = binary_path.resolve()
    evidence["binary_path"] = str(binary_path)
    if not binary_path.is_file():
        evidence["reason"] = "binary_manifest_binary_missing"
        return evidence
    if not isinstance(expected_hash, str) or not re.fullmatch(r"[0-9a-fA-F]{64}", expected_hash):
        evidence["reason"] = "binary_manifest_binary_hash_missing"
        return evidence
    try:
        actual_hash = sha256_file(binary_path)
    except OSError as error:
        evidence["reason"] = f"binary_manifest_binary_unreadable:{type(error).__name__}"
        return evidence
    evidence["binary_sha256"] = actual_hash
    if actual_hash.lower() != expected_hash.lower():
        evidence["reason"] = "binary_manifest_binary_hash_mismatch"
        return evidence
    if (
        not isinstance(compiler, str)
        or not compiler.strip()
        or not isinstance(profile, str)
        or not profile.strip()
        or flags is None
    ):
        evidence["reason"] = "binary_manifest_build_provenance_missing"
        return evidence
    if isinstance(flags, str):
        flags = [flags]
    if not isinstance(flags, list) or any(not isinstance(flag, str) for flag in flags):
        evidence["reason"] = "binary_manifest_build_flags_invalid"
        return evidence
    try:
        manifest_hash = sha256_file(manifest_path)
    except OSError as error:
        evidence["reason"] = f"binary_manifest_unreadable:{type(error).__name__}"
        return evidence
    evidence.update(
        {
            "status": "verified",
            "reason": None,
            "manifest_sha256": manifest_hash,
            "compiler": compiler,
            "profile": profile,
            "flags": flags,
        }
    )
    return evidence


def service_command_binary_binding(
    command_template: str, provenance: dict[str, Any]
) -> dict[str, str]:
    if provenance.get("status") != "verified":
        return {"status": "unavailable", "reason": "binary_provenance_unavailable"}
    if "{binary}" in command_template:
        return {"status": "explicit_placeholder", "source": "{binary}"}
    if "$PERF_LOCAL_BINARY_PATH" in command_template or "${PERF_LOCAL_BINARY_PATH}" in command_template:
        return {"status": "explicit_environment", "source": "PERF_LOCAL_BINARY_PATH"}
    return {"status": "unbound", "reason": "service_command_binary_binding_missing"}


def unavailable_service_process(
    reason: str, launcher_pid: int | None = None
) -> dict[str, Any]:
    return {
        "status": "unavailable",
        "reason": reason,
        "launcher_pid": launcher_pid,
        "service_pid": None,
        "service_exe": None,
        "service_sha256": None,
        "descendant_pids": [],
        "candidate_processes": [],
        "errors": [reason],
    }


def process_descendant_observation(
    root_pid: int, proc_root: Path = Path("/proc")
) -> dict[str, Any]:
    """Bound process-tree discovery to the launcher and its descendants."""

    queue = [root_pid]
    seen = {root_pid}
    descendants: list[int] = []
    errors: list[str] = []
    truncated = False
    while queue:
        parent_pid = queue.pop(0)
        children_path = proc_root / str(parent_pid) / "task" / str(parent_pid) / "children"
        try:
            children = children_path.read_text(encoding="ascii").split()
        except (FileNotFoundError, OSError, UnicodeError) as error:
            errors.append(f"pid_{parent_pid}_children:{type(error).__name__}")
            continue
        for child_text in children:
            try:
                child_pid = int(child_text)
            except ValueError:
                errors.append(f"pid_{parent_pid}_child_invalid")
                continue
            if child_pid <= 0 or child_pid in seen:
                continue
            if len(descendants) >= MAX_SERVICE_PROCESS_DESCENDANTS:
                truncated = True
                break
            seen.add(child_pid)
            descendants.append(child_pid)
            queue.append(child_pid)
        if truncated:
            break
    return {
        "root_pid": root_pid,
        "descendant_pids": descendants,
        "errors": errors,
        "truncated": truncated,
    }


def process_group_observation(group_id: int) -> dict[str, Any]:
    """Observe members of one process group without widening to unrelated PIDs."""

    members: set[int] = set()
    errors: list[str] = []
    proc_root = Path("/proc")
    if proc_root.is_dir():
        try:
            entries = list(proc_root.iterdir())
        except OSError as error:
            entries = []
            errors.append(f"proc_iter:{type(error).__name__}")
        for entry in entries:
            if not entry.name.isdigit():
                continue
            pid = int(entry.name)
            try:
                if os.getpgid(pid) == group_id:
                    members.add(pid)
            except (FileNotFoundError, ProcessLookupError, PermissionError, OSError):
                continue
    else:
        try:
            completed = subprocess.run(
                ["ps", "-eo", "pid=,pgid="],
                capture_output=True,
                text=True,
                check=False,
            )
            if completed.returncode != 0:
                errors.append(f"ps_exit_{completed.returncode}")
            for line in completed.stdout.splitlines():
                fields = line.split()
                if len(fields) != 2:
                    continue
                try:
                    pid, observed_group = (int(value) for value in fields)
                except ValueError:
                    continue
                if observed_group == group_id:
                    members.add(pid)
        except OSError as error:
            errors.append(f"ps:{type(error).__name__}")
    return {
        "group_id": group_id,
        "pids": sorted(members),
        "errors": errors,
    }


def _cleanup_process_ids(process: subprocess.Popen[bytes], group_id: int) -> list[int]:
    group = process_group_observation(group_id)
    observed = set(group["pids"])
    if process.poll() is None:
        observed.add(process.pid)
    return sorted(observed)


def service_process_binary_binding(
    launcher_pid: int,
    provenance: dict[str, Any],
    proc_root: Path = Path("/proc"),
) -> dict[str, Any]:
    """Bind the ready listener to a live process whose executable matches the build manifest."""

    expected_hash = provenance.get("binary_sha256")
    if provenance.get("status") != "verified" or not isinstance(expected_hash, str):
        return unavailable_service_process("binary_provenance_unavailable", launcher_pid)

    process_tree = process_descendant_observation(launcher_pid, proc_root)
    candidate_pids = [launcher_pid, *process_tree["descendant_pids"]]
    candidates: list[dict[str, Any]] = []
    matched: dict[str, Any] | None = None
    for pid in candidate_pids[:MAX_SERVICE_PROCESS_CANDIDATES]:
        candidate: dict[str, Any] = {
            "pid": pid,
            "role": "launcher" if pid == launcher_pid else "descendant",
            "exe": None,
            "sha256": None,
            "matches_manifest": False,
        }
        try:
            proc_exe = proc_root / str(pid) / "exe"
            raw_exe = os.readlink(proc_exe)
            exe = raw_exe.removesuffix(" (deleted)")
            candidate["exe"] = exe
            # /proc/<pid>/exe follows the executing inode even when its displayed pathname
            # has been replaced or unlinked.
            actual_hash = sha256_file(proc_exe)
            candidate["sha256"] = actual_hash
            candidate["matches_manifest"] = actual_hash.lower() == expected_hash.lower()
            if candidate["matches_manifest"] and matched is None:
                matched = candidate
        except (FileNotFoundError, OSError, UnicodeError) as error:
            candidate["error"] = type(error).__name__
        candidates.append(candidate)

    if matched is None:
        evidence = unavailable_service_process("service_binary_process_not_found", launcher_pid)
        evidence.update(
            {
                "descendant_pids": process_tree["descendant_pids"],
                "candidate_processes": candidates,
                "errors": [
                    *process_tree["errors"],
                    *(["process_tree_truncated"] if process_tree["truncated"] else []),
                    "service_binary_process_not_found",
                ],
            }
        )
        return evidence

    launcher = next((candidate for candidate in candidates if candidate["pid"] == launcher_pid), None)
    return {
        "status": "verified",
        "reason": None,
        "launcher_pid": launcher_pid,
        "launcher": launcher,
        "service_pid": matched["pid"],
        "service_exe": matched["exe"],
        "service_sha256": matched["sha256"],
        "manifest_binary_path": provenance.get("binary_path"),
        "manifest_binary_sha256": expected_hash,
        "descendant_pids": process_tree["descendant_pids"],
        "candidate_processes": candidates,
        "errors": [
            *process_tree["errors"],
            *(["process_tree_truncated"] if process_tree["truncated"] else []),
        ],
    }


def parse_proc_cpu_stat(text: str) -> dict[str, int]:
    line = next((line for line in text.splitlines() if line.startswith("cpu ")), "")
    fields = line.split()
    if len(fields) < 9:
        raise ValueError("cpu_stat_missing")
    names = ("user", "nice", "system", "idle", "iowait", "irq", "softirq", "steal", "guest", "guest_nice")
    values = [int(value) for value in fields[1 : 1 + len(names)]]
    return dict(zip(names, values, strict=False))


def parse_proc_psi(text: str) -> dict[str, dict[str, float]]:
    result: dict[str, dict[str, float]] = {}
    for line in text.splitlines():
        fields = line.split()
        if not fields:
            continue
        values: dict[str, float] = {}
        for field_value in fields[1:]:
            key, separator, value = field_value.partition("=")
            if separator and key in {"avg10", "avg60", "avg300"}:
                values[key] = float(value)
        if values:
            result[fields[0]] = values
    if not result:
        raise ValueError("psi_missing")
    return result


def parse_proc_loadavg(text: str) -> dict[str, float | int]:
    fields = text.split()
    if len(fields) < 4:
        raise ValueError("loadavg_missing")
    runnable, separator, total = fields[3].partition("/")
    if not separator:
        raise ValueError("loadavg_runnable_missing")
    return {
        "load_1m": float(fields[0]),
        "load_5m": float(fields[1]),
        "load_15m": float(fields[2]),
        "runnable": int(runnable),
        "processes": int(total),
        "last_pid": int(fields[4]) if len(fields) > 4 else 0,
    }


def parse_proc_process_stat(text: str, clock_ticks: int) -> dict[str, int]:
    closing_parenthesis = text.rfind(")")
    if closing_parenthesis < 0:
        raise ValueError("process_stat_missing")
    fields = text[closing_parenthesis + 2 :].split()
    if len(fields) < 22:
        raise ValueError("process_stat_fields_missing")
    return {
        "utime_ticks": int(fields[11]),
        "stime_ticks": int(fields[12]),
        "rss_bytes": int(fields[21]) * os.sysconf("SC_PAGE_SIZE"),
        "clock_ticks": clock_ticks,
    }


def parse_proc_process_io(text: str) -> dict[str, int]:
    result: dict[str, int] = {}
    for line in text.splitlines():
        key, separator, value = line.partition(":")
        if separator:
            try:
                result[key.strip()] = int(value.strip())
            except ValueError:
                continue
    if not result:
        raise ValueError("process_io_missing")
    return result


def parse_proc_diskstats(text: str) -> dict[str, dict[str, int]]:
    result: dict[str, dict[str, int]] = {}
    for line in text.splitlines():
        fields = line.split()
        if len(fields) < 14:
            continue
        try:
            result[fields[2]] = {
                "reads_completed": int(fields[3]),
                "sectors_read": int(fields[5]),
                "writes_completed": int(fields[7]),
                "sectors_written": int(fields[9]),
                "io_ms": int(fields[12]),
                "weighted_io_ms": int(fields[13]),
            }
        except ValueError:
            continue
    if not result:
        raise ValueError("diskstats_missing")
    return dict(list(result.items())[:MAX_SYSTEM_DIAGNOSTIC_DEVICES])


def collect_system_snapshot(pid: int, proc_root: Path = Path("/proc")) -> dict[str, Any]:
    errors: list[str] = []

    def read(path: Path, label: str) -> str | None:
        try:
            return path.read_text(encoding="ascii")
        except (FileNotFoundError, OSError, UnicodeError) as error:
            errors.append(f"{label}:{type(error).__name__}")
            return None

    cpu_text = read(proc_root / "stat", "cpu")
    psi_cpu_text = read(proc_root / "pressure" / "cpu", "psi_cpu")
    psi_io_text = read(proc_root / "pressure" / "io", "psi_io")
    load_text = read(proc_root / "loadavg", "load")
    process_stat_text = read(proc_root / str(pid) / "stat", "process_stat")
    process_io_text = read(proc_root / str(pid) / "io", "process_io")
    diskstats_text = read(proc_root / "diskstats", "diskstats")
    snapshot: dict[str, Any] = {"captured_at_unix": time.time()}
    try:
        if cpu_text is not None:
            snapshot["cpu"] = parse_proc_cpu_stat(cpu_text)
    except (TypeError, ValueError) as error:
        errors.append(f"cpu_parse:{type(error).__name__}")
    for key, text, label in (
        ("psi_cpu", psi_cpu_text, "psi_cpu"),
        ("psi_io", psi_io_text, "psi_io"),
    ):
        try:
            if text is not None:
                snapshot[key] = parse_proc_psi(text)
        except (TypeError, ValueError) as error:
            errors.append(f"{label}_parse:{type(error).__name__}")
    try:
        if load_text is not None:
            snapshot["load"] = parse_proc_loadavg(load_text)
    except (TypeError, ValueError) as error:
        errors.append(f"load_parse:{type(error).__name__}")
    try:
        if process_stat_text is not None:
            snapshot["process"] = parse_proc_process_stat(
                process_stat_text, int(os.sysconf("SC_CLK_TCK"))
            )
    except (TypeError, ValueError, OSError) as error:
        errors.append(f"process_stat_parse:{type(error).__name__}")
    try:
        if process_io_text is not None:
            snapshot["process_io"] = parse_proc_process_io(process_io_text)
    except (TypeError, ValueError) as error:
        errors.append(f"process_io_parse:{type(error).__name__}")
    try:
        if diskstats_text is not None:
            snapshot["storage"] = parse_proc_diskstats(diskstats_text)
    except (TypeError, ValueError) as error:
        errors.append(f"diskstats_parse:{type(error).__name__}")
    snapshot["errors"] = errors
    return snapshot


class SystemDiagnosticsCollector:
    """Collect low-frequency, same-cadence guest diagnostics without changing limits."""

    def __init__(
        self,
        pid: int,
        interval_seconds: float = DEFAULT_SYSTEM_DIAGNOSTICS_INTERVAL_SECONDS,
        proc_root: Path = Path("/proc"),
    ) -> None:
        self.pid = pid
        self.interval_seconds = interval_seconds
        self.proc_root = proc_root
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self._samples: list[dict[str, Any]] = []
        self._errors: list[str] = []
        self._previous_cpu: dict[str, int] | None = None
        self._dropped_samples = 0

    def _capture(self) -> None:
        try:
            snapshot = collect_system_snapshot(self.pid, self.proc_root)
        except (OSError, TypeError, ValueError) as error:
            snapshot = {
                "captured_at_unix": time.time(),
                "errors": [f"collector_capture:{type(error).__name__}"],
            }
        cpu = snapshot.get("cpu")
        if isinstance(cpu, dict):
            if self._previous_cpu is not None:
                total_fields = (
                    "user",
                    "nice",
                    "system",
                    "idle",
                    "iowait",
                    "irq",
                    "softirq",
                    "steal",
                )
                total_delta = sum(
                    int(cpu.get(name, 0)) - int(self._previous_cpu.get(name, 0))
                    for name in total_fields
                )
                steal_delta = int(cpu.get("steal", 0)) - int(self._previous_cpu.get("steal", 0))
                snapshot["cpu_delta"] = {
                    "total_jiffies": max(0, total_delta),
                    "steal_jiffies": max(0, steal_delta),
                    "steal_percent": (
                        round(max(0, steal_delta) / total_delta * 100, 6)
                        if total_delta > 0
                        else None
                    ),
                }
            self._previous_cpu = cpu
        if len(self._samples) >= MAX_SYSTEM_DIAGNOSTIC_SAMPLES:
            self._dropped_samples += 1
        else:
            self._samples.append(snapshot)
        self._errors.extend(snapshot.get("errors", []))

    def _run(self) -> None:
        while not self._stop.wait(self.interval_seconds):
            self._capture()

    def start(self) -> None:
        self._capture()
        self._thread = threading.Thread(target=self._run, name="perf-system-diagnostics", daemon=True)
        self._thread.start()

    def stop(self) -> None:
        if self._thread is None:
            return
        self._stop.set()
        thread = self._thread
        thread.join(timeout=max(1.0, self.interval_seconds + 1.0))
        if thread.is_alive():
            if "collector_join_timeout" not in self._errors:
                self._errors.append("collector_join_timeout")
            return
        self._thread = None

    def evidence(self) -> dict[str, Any]:
        required = (
            "cpu",
            "cpu_delta",
            "psi_cpu",
            "psi_io",
            "load",
            "process",
            "process_io",
            "storage",
        )
        observed = {key for sample in self._samples for key in sample}
        missing = [key for key in required if key not in observed]
        status = "validated" if self._samples and not missing and not self._errors else "unavailable"
        reason = (
            "collector_join_timeout"
            if "collector_join_timeout" in self._errors
            else None if status == "validated" else "system_diagnostics_incomplete"
        )
        return {
            "status": status,
            "reason": reason,
            "pid": self.pid,
            "proc_root": str(self.proc_root),
            "cadence_seconds": self.interval_seconds,
            "sample_count": len(self._samples),
            "dropped_samples": self._dropped_samples,
            "missing": missing,
            "errors": sorted(set(self._errors)),
            "samples": self._samples,
            "joined": self._thread is None or not self._thread.is_alive(),
        }


def unavailable_system_diagnostics(reason: str) -> dict[str, Any]:
    return {
        "status": "unavailable",
        "reason": reason,
        "cadence_seconds": DEFAULT_SYSTEM_DIAGNOSTICS_INTERVAL_SECONDS,
        "sample_count": 0,
        "dropped_samples": 0,
        "missing": [
            "process",
            "process_io",
            "cpu",
            "cpu_delta",
            "psi_cpu",
            "psi_io",
            "load",
            "storage",
        ],
        "errors": [reason],
        "samples": [],
    }


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
        self.contract_scenarios = contract.get("scenarios")
        self.variant_root = self.root / "variants"
        self.pair_root = self.root / "pairs"
        self.phase_root = self.root / "phases"
        self.log_root = self.root / "logs"
        self.variant_root.mkdir(parents=True, exist_ok=True)
        self.pair_root.mkdir(parents=True, exist_ok=True)
        self.phase_root.mkdir(parents=True, exist_ok=True)
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
                "phase_observation": None,
                "scenarios": None,
                "scenario_contract": self.contract_scenarios,
                "credential_mapping": None,
                "resource_observation": None,
                "binary_provenance": {
                    "status": "pending",
                    "reason": "variant_not_started",
                },
                "service_command_binding": {
                    "status": "pending",
                    "reason": "variant_not_started",
                },
                "service_process": unavailable_service_process("variant_not_started"),
                "system_diagnostics": unavailable_system_diagnostics("variant_not_started"),
                "logs": None,
                "cleanup": None,
                "workspace": None,
                "error": None,
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
            "success_request": result.get("success_request"),
            "unrelated_cohort": result.get("unrelated_cohort"),
            "statuses": result.get("statuses", {}),
            "business_statuses": result.get("business_statuses", {}),
            "outcome_counts": result.get("outcome_counts", {}),
            "business_outcomes": result.get("business_outcomes", {}),
            "response_evidence": result.get("response_evidence", []),
            "errors": result.get("errors", {}),
            "phases": result.get("phases", {}),
            "phase_schema": result.get("phase_schema"),
            "phase_observation": result.get("phase_observation"),
            "warmup": result.get("warmup"),
            "fixture_settings": result.get("fixture_settings"),
            "mock_upstream": result.get("mock_upstream"),
            "cohort": result.get("cohort"),
            "accepted_progress": result.get("accepted_progress"),
            "scenarios": result.get("scenarios"),
            "scenario_contract": result.get("scenario_contract"),
            "credential_mapping": result.get("credential_mapping"),
            "resource_observation": result.get("resource_observation"),
            "binary_provenance": result.get("binary_provenance"),
            "service_command_binding": result.get("service_command_binding"),
            "service_process": result.get("service_process"),
            "system_diagnostics": result.get("system_diagnostics"),
            "logs": result.get("logs"),
            "cleanup": result.get("cleanup"),
            "workspace": result.get("workspace"),
        }
        if result.get("error_type") is not None:
            payload["error_type"] = result["error_type"]
        if result.get("error") is not None:
            payload["error"] = result["error"]
        if result.get("failure_stage") is not None:
            payload["failure_stage"] = result["failure_stage"]
        if result.get("blocked_by") is not None:
            payload["blocked_by"] = result["blocked_by"]
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

    def write_phase_comparison(
        self,
        phase: str,
        rounds: list[dict[str, Any]],
        comparison: dict[str, Any],
    ) -> None:
        atomic_write_json(
            self.phase_root / f"{phase}-comparison.json",
            {
                "kind": "phase_comparison",
                "phase": phase,
                "status": comparison.get("status"),
                "reasons": comparison.get("reasons", []),
                "rounds": rounds,
                "comparison": comparison,
            },
        )
        self._write_progress(
            "phase_complete",
            phase=phase,
            phase_status=comparison.get("status"),
            reasons=comparison.get("reasons", []),
        )

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
                "phase_observation": None,
                "scenarios": None,
                "scenario_contract": self.contract_scenarios,
                "credential_mapping": None,
                "resource_observation": None,
                "binary_provenance": {
                    "status": "missing",
                    "reason": "interrupted_before_completion",
                },
                "service_command_binding": {
                    "status": "unavailable",
                    "reason": "interrupted_before_completion",
                },
                "service_process": unavailable_service_process(
                    "interrupted_before_completion"
                ),
                "system_diagnostics": unavailable_system_diagnostics(
                    "interrupted_before_completion"
                ),
                "logs": None,
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
    success_elapsed_ms: list[float] = field(default_factory=list)
    unrelated_elapsed_ms: list[float] = field(default_factory=list)
    statuses: dict[str, int] = field(default_factory=dict)
    outcome_counts: dict[str, int] = field(default_factory=dict)
    response_evidence: dict[str, dict[str, Any]] = field(default_factory=dict)
    errors: dict[str, int] = field(default_factory=dict)
    accepted_progress: int = 0
    reported_progress: int = 0
    _lock: threading.Lock = field(default_factory=threading.Lock, repr=False)

    @staticmethod
    def _outcome_for_status(status: int) -> str:
        if 200 <= status < 300:
            return "success"
        if status == 429:
            return "reject"
        return "error"

    @staticmethod
    def _bounded_value(value: object, limit: int = 160) -> str | None:
        if value is None:
            return None
        text = redact_output(str(value)).strip()
        if not text:
            return None
        return text[:limit]

    def _record_response_evidence(
        self,
        status: int,
        decoded: object,
        headers: dict[str, str] | None,
    ) -> None:
        if status != 429:
            return
        body_fields: dict[str, str] = {}
        if isinstance(decoded, dict):
            for field_name in ("error", "code", "reason", "message"):
                value = self._bounded_value(decoded.get(field_name))
                if value is not None:
                    body_fields[field_name] = value
        if not body_fields:
            body_fields["body"] = "invalid_or_unstructured_json"
        safe_headers: dict[str, str] = {}
        for name, value in (headers or {}).items():
            normalized_name = name.lower()
            if (
                normalized_name == "content-type"
                or normalized_name == "retry-after"
                or normalized_name.startswith("x-ratelimit-")
                or normalized_name.startswith("ratelimit-")
            ):
                bounded = self._bounded_value(value)
                if bounded is not None:
                    safe_headers[normalized_name] = bounded
        signature = json.dumps(
            {"body": body_fields, "headers": safe_headers},
            sort_keys=True,
            separators=(",", ":"),
        )
        entry = self.response_evidence.get(signature)
        if entry is None:
            if len(self.response_evidence) >= MAX_RESPONSE_EVIDENCE:
                return
            self.response_evidence[signature] = {
                "status": status,
                "count": 1,
                "body": body_fields,
                "headers": safe_headers,
            }
        else:
            entry["count"] = int(entry["count"]) + 1

    def add_status(
        self,
        lane: str,
        status: int,
        elapsed_ms: float,
        body: bytes,
        headers: dict[str, str] | None = None,
    ) -> None:
        try:
            decoded = json.loads(body)
        except (ValueError, UnicodeDecodeError):
            decoded = None
        with self._lock:
            key = f"{lane}:{status}"
            self.statuses[key] = self.statuses.get(key, 0) + 1
            outcome = self._outcome_for_status(status)
            outcome_key = f"{lane}:{outcome}"
            self.outcome_counts[outcome_key] = self.outcome_counts.get(outcome_key, 0) + 1
            target = self.unrelated_elapsed_ms if lane == "unrelated" else self.elapsed_ms
            target.append(elapsed_ms)
            if lane == "business" and outcome == "success":
                self.success_elapsed_ms.append(elapsed_ms)
            if lane == "business":
                self._record_response_evidence(status, decoded, headers)
            # HTTP 200 from the business lane is not maintenance progress. Only a
            # maintenance/recovery response explicitly carrying accepted progress counts.
            if lane in {"maintenance", "recovery"} and 200 <= status < 300:
                self.accepted_progress += 1
            if lane in {"maintenance", "recovery"} and isinstance(decoded, dict):
                value = decoded.get("accepted_progress")
                if isinstance(value, int) and value >= 0:
                    self.reported_progress += value

    def add_error(self, lane: str, error: BaseException) -> None:
        with self._lock:
            key = f"{lane}:{type(error).__name__}"
            self.errors[key] = self.errors.get(key, 0) + 1
            outcome_key = f"{lane}:error"
            self.outcome_counts[outcome_key] = self.outcome_counts.get(outcome_key, 0) + 1

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
        with self._lock:
            elapsed_ms = list(self.elapsed_ms)
            success_elapsed_ms = list(self.success_elapsed_ms)
            unrelated_elapsed_ms = list(self.unrelated_elapsed_ms)
            statuses = dict(self.statuses)
            outcome_counts = dict(self.outcome_counts)
            response_evidence = [
                dict(entry)
                for entry in self.response_evidence.values()
            ]
            errors = dict(self.errors)
            accepted_progress = self.accepted_progress
            reported_progress = self.reported_progress
        business_statuses = {"200": 0, "429": 0, "other": 0}
        for key, count in statuses.items():
            lane, _, raw_status = key.partition(":")
            if lane != "business":
                continue
            bucket = raw_status if raw_status in {"200", "429"} else "other"
            business_statuses[bucket] += count
        return {
            "duration_seconds": duration_seconds,
            "request": self.stats(elapsed_ms, duration_seconds),
            "success_request": self.stats(success_elapsed_ms, duration_seconds),
            "unrelated_cohort": self.stats(unrelated_elapsed_ms, duration_seconds),
            "statuses": dict(sorted(statuses.items())),
            "business_statuses": business_statuses,
            "outcome_counts": dict(sorted(outcome_counts.items())),
            "business_outcomes": {
                outcome: outcome_counts.get(f"business:{outcome}", 0)
                for outcome in ("success", "reject", "error")
            },
            "response_evidence": sorted(
                response_evidence,
                key=lambda entry: (-int(entry["count"]), json.dumps(entry, sort_keys=True)),
            ),
            "errors": dict(sorted(errors.items())),
            "phases": {},
            "accepted_progress": {
                "source": "maintenance_or_recovery_response_field_only",
                "responses": accepted_progress,
                "reported": reported_progress,
            },
        }

    def merge(self, other: "SampleRecorder") -> None:
        with other._lock:
            elapsed_ms = list(other.elapsed_ms)
            success_elapsed_ms = list(other.success_elapsed_ms)
            unrelated_elapsed_ms = list(other.unrelated_elapsed_ms)
            statuses = dict(other.statuses)
            outcome_counts = dict(other.outcome_counts)
            response_evidence = {
                signature: dict(entry)
                for signature, entry in other.response_evidence.items()
            }
            errors = dict(other.errors)
            accepted_progress = other.accepted_progress
            reported_progress = other.reported_progress
        with self._lock:
            self.elapsed_ms.extend(elapsed_ms)
            self.success_elapsed_ms.extend(success_elapsed_ms)
            self.unrelated_elapsed_ms.extend(unrelated_elapsed_ms)
            for key, value in statuses.items():
                self.statuses[key] = self.statuses.get(key, 0) + value
            for key, value in outcome_counts.items():
                self.outcome_counts[key] = self.outcome_counts.get(key, 0) + value
            for signature, entry in response_evidence.items():
                if signature in self.response_evidence:
                    self.response_evidence[signature]["count"] += int(entry["count"])
                elif len(self.response_evidence) < MAX_RESPONSE_EVIDENCE:
                    self.response_evidence[signature] = entry
            for key, value in errors.items():
                self.errors[key] = self.errors.get(key, 0) + value
            self.accepted_progress += accepted_progress
            self.reported_progress += reported_progress


def create_fixture(directory: Path) -> tuple[Path, Path]:
    directory.mkdir(parents=True, exist_ok=True)
    core = directory / "tavily_proxy.db"
    sidecar = directory / "tavily_proxy-observability.db"
    # Leave both databases schema-empty so the service owns migration/bootstrap state.
    for path in (core, sidecar):
        with closing(sqlite3.connect(path)):
            pass
    core.chmod(0o600)
    sidecar.chmod(0o600)
    return core, sidecar


def extract_archive_safely(stream: tarfile.TarFile, destination: Path) -> None:
    if sys.version_info >= (3, 12):
        stream.extractall(destination, filter="data")
        return

    destination_path = os.path.realpath(destination)
    for member in stream:
        member_name = member.name
        if not member_name:
            raise ValueError("archive member path is empty")
        if os.path.isabs(member_name) or Path(member_name).is_absolute():
            raise ValueError(f"archive member path is absolute: {member_name!r}")
        if member.issym() or member.islnk():
            raise ValueError(f"archive member links are not allowed: {member_name!r}")
        if not (member.isfile() or member.isdir()):
            raise ValueError(f"archive member special files are not allowed: {member_name!r}")

        target_path = os.path.realpath(os.path.join(destination_path, member_name))
        try:
            within_destination = os.path.commonpath([target_path, destination_path]) == destination_path
        except ValueError as error:
            raise ValueError(f"archive member path escapes destination: {member_name!r}") from error
        if not within_destination:
            raise ValueError(f"archive member path escapes destination: {member_name!r}")

        safe_mode: int | None = None
        if member.mode is not None:
            mode = member.mode & 0o755
            if member.isfile():
                if not mode & 0o100:
                    mode &= ~0o111
                mode |= 0o600
            else:
                mode = None
            safe_mode = mode
        stream.extract(member, destination, set_attrs=False)
        if safe_mode is not None and member.isfile():
            os.chmod(target_path, safe_mode)


def archive_revision(root: Path, revision: str, destination: Path) -> str:
    resolved = subprocess.check_output(
        ["git", "-C", str(root), "rev-parse", "--verify", f"{revision}^{{commit}}"],
        text=True,
    ).strip()
    archive: subprocess.Popen[bytes] | None = None
    try:
        archive = subprocess.Popen(
            ["git", "-C", str(root), "archive", resolved], stdout=subprocess.PIPE
        )
        assert archive.stdout is not None
        with tarfile.open(fileobj=archive.stdout, mode="r|") as stream:
            extract_archive_safely(stream, destination)
        if archive.wait() != 0:
            raise RuntimeError(f"unable to archive revision {revision}")
        return resolved
    except BaseException:
        if archive is not None and archive.poll() is None:
            archive.terminate()
            try:
                archive.wait(timeout=2)
            except subprocess.TimeoutExpired:
                archive.kill()
                archive.wait(timeout=2)
        raise
    finally:
        if archive is not None and archive.stdout is not None:
            archive.stdout.close()


def find_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.bind(("127.0.0.1", 0))
        return int(probe.getsockname()[1])


def wait_for_service(port: int, timeout_seconds: float) -> dict[str, Any]:
    started = time.monotonic()
    deadline = time.monotonic() + timeout_seconds
    attempts = 0
    while time.monotonic() < deadline:
        connection: http.client.HTTPConnection | None = None
        try:
            connection = http.client.HTTPConnection("127.0.0.1", port, timeout=1)
            connection.request("GET", "/health")
            response = connection.getresponse()
            response.read()
            attempts += 1
            return {
                "status": "listener_ready",
                "endpoint": "/health",
                "listener_ready": True,
                "health_ready": 200 <= response.status < 300,
                "http_status": response.status,
                "attempts": attempts,
                "elapsed_ms": round((time.monotonic() - started) * 1000, 3),
            }
        except (OSError, http.client.HTTPException, TimeoutError):
            attempts += 1
        finally:
            if connection is not None:
                connection.close()
        time.sleep(0.25)
    return {
        "status": "listener_timeout",
        "endpoint": "/health",
        "listener_ready": False,
        "health_ready": False,
        "http_status": None,
        "attempts": attempts,
        "elapsed_ms": round((time.monotonic() - started) * 1000, 3),
    }


def configure_synthetic_fixture(port: int) -> dict[str, Any]:
    request_rate_limit = int(SYNTHETIC_FIXTURE_SETTINGS["REQUEST_RATE_LIMIT"])
    affinity_key_count = int(SYNTHETIC_FIXTURE_SETTINGS["MCP_SESSION_AFFINITY_KEY_COUNT"])
    payload = json.dumps(
        {
            "requestRateLimit": request_rate_limit,
            "mcpSessionAffinityKeyCount": affinity_key_count,
        }
    ).encode("utf-8")
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
    try:
        connection.request(
            "PUT",
            "/api/settings/system",
            body=payload,
            headers={"Content-Type": "application/json"},
        )
        response = connection.getresponse()
        response_body = response.read()
    except (OSError, http.client.HTTPException, TimeoutError) as error:
        raise RuntimeError("synthetic_fixture_settings_transport") from error
    finally:
        connection.close()
    if response.status != 200:
        raise RuntimeError(f"synthetic_fixture_settings_status_{response.status}")
    try:
        persisted = json.loads(response_body)
    except (ValueError, UnicodeDecodeError) as error:
        raise RuntimeError("synthetic_fixture_settings_invalid_json") from error
    if persisted.get("requestRateLimit") != request_rate_limit:
        raise RuntimeError("synthetic_fixture_settings_request_rate_limit_mismatch")
    if persisted.get("mcpSessionAffinityKeyCount") != affinity_key_count:
        raise RuntimeError("synthetic_fixture_settings_affinity_key_count_mismatch")
    return {
        "request_rate_limit_status": "applied",
        "request_rate_limit": request_rate_limit,
        "request_rate_limit_window_minutes": int(
            SYNTHETIC_FIXTURE_SETTINGS["REQUEST_RATE_LIMIT_WINDOW_MINUTES"]
        ),
        "mcp_session_affinity_key_count": affinity_key_count,
    }


def collect_phase_snapshot(port: int, token: str) -> dict[str, Any]:
    empty_observability = {"dropped": {}, "flush": {}}
    empty_counters = {"http_requests": {}}
    unavailable = {
        "status": "unsupported",
        "kind": PHASE_SCHEMA_UNSUPPORTED,
        "reason": "metrics_unavailable",
        "metric_name": None,
        "selector": None,
        "phase_keys": [],
    }
    try:
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=3)
        connection.request("GET", "/metrics", headers={"Authorization": f"Bearer {token}"})
        response = connection.getresponse()
        body = response.read().decode("utf-8", errors="replace")
        connection.close()
    except (OSError, http.client.HTTPException, TimeoutError):
        return {
            "values": {},
            "schema": unavailable,
            "counters": empty_counters,
            "observability": empty_observability,
        }
    if response.status != 200:
        return {
            "values": {},
            "schema": {
                **unavailable,
                "reason": f"metrics_http_status_{response.status}",
                "http_status": response.status,
            },
            "counters": empty_counters,
            "observability": empty_observability,
        }
    values_by_schema: dict[str, dict[str, dict[str, float]]] = {
        PHASE_SCHEMA_LEGACY: {},
        PHASE_SCHEMA_OPERATION: {},
    }
    observability = {
        "dropped": parse_counter_family(
            body, "hikari_observability_dropped_total", ("reason",)
        ),
        "flush": parse_counter_family(
            body, "hikari_observability_flush_total", ("outcome", "reason")
        ),
    }
    counters = {
        "http_requests": parse_counter_family(
            body,
            "hikari_http_requests_total",
            ("route_family", "method", "status_class", "outcome"),
        )
    }
    for line in body.splitlines():
        metric_name = next(
            (
                name
                for name in PHASE_METRIC_NAMES.values()
                if line.startswith(f"{name}_")
            ),
            None,
        )
        if metric_name is None:
            continue
        match = re.match(
            rf"^{re.escape(metric_name)}_(count|sum)\{{([^}}]*)\}}\s+([0-9.eE+-]+)$",
            line,
        )
        if not match:
            continue
        metric_kind, labels_text, raw_value = match.groups()
        labels = dict(re.findall(r'(\w+)="([^"]*)"', labels_text))
        schema_kind = next(
            kind for kind, name in PHASE_METRIC_NAMES.items() if name == metric_name
        )
        if not phase_metric_matches_schema(labels, schema_kind):
            continue
        phase = labels.get("phase")
        if phase is None:
            continue
        values = values_by_schema[schema_kind]
        bucket = values.setdefault(phase, {"count": 0.0, "sum_seconds": 0.0})
        bucket["count" if metric_kind == "count" else "sum_seconds"] = float(raw_value)
    selected_kind = next(
        (
            kind
            for kind in (PHASE_SCHEMA_OPERATION, PHASE_SCHEMA_LEGACY)
            if values_by_schema[kind]
        ),
        None,
    )
    if selected_kind is None:
        schema = {
            **unavailable,
            "reason": "selector_no_matching_phase_series",
            "http_status": response.status,
        }
        return {
            "values": {},
            "schema": schema,
            "counters": counters,
            "observability": observability,
        }
    selector = (
        PHASE_METRIC_SELECTOR
        if selected_kind == PHASE_SCHEMA_OPERATION
        else LEGACY_PHASE_METRIC_SELECTOR
    )
    schema = {
        "status": "supported",
        "kind": selected_kind,
        "metric_name": PHASE_METRIC_NAMES[selected_kind],
        "selector": dict(selector),
        "phase_keys": sorted(values_by_schema[selected_kind]),
        "http_status": response.status,
    }
    return {
        "values": values_by_schema[selected_kind],
        "schema": schema,
        "counters": counters,
        "observability": observability,
    }


def parse_counter_family(
    body: str, metric_name: str, label_names: tuple[str, ...]
) -> dict[str, float]:
    values: dict[str, float] = {}
    pattern = re.compile(
        rf"^{re.escape(metric_name)}\{{([^}}]*)\}}\s+([0-9.eE+-]+)$"
    )
    for line in body.splitlines():
        match = pattern.match(line)
        if match is None:
            continue
        labels = dict(re.findall(r'(\w+)="([^"]*)"', match.group(1)))
        if any(name not in labels for name in label_names):
            continue
        key = "|".join(labels[name] for name in label_names)
        values[key] = float(match.group(2))
    return dict(sorted(values.items()))


class MetricsCollector:
    """Capture bounded structured metric snapshots around a workload window."""

    def __init__(self, port: int, token: str, interval_seconds: float) -> None:
        self.port = port
        self.token = token
        self.interval_seconds = interval_seconds
        self._stop_event = threading.Event()
        self._scrape_lock = threading.Lock()
        self._history_lock = threading.Lock()
        self._thread: threading.Thread | None = None
        self._snapshots: list[dict[str, Any]] = []
        self._errors: list[dict[str, Any]] = []
        self._sequence = 0
        self._scrape_attempts = 0
        self._scrape_successes = 0
        self._scrape_failures = 0
        self._scrape_failure_reasons: dict[str, int] = {}
        self._history_truncated_count = 0

    def start(self) -> None:
        if self.interval_seconds <= 0 or self._thread is not None:
            return
        self._thread = threading.Thread(target=self._run, daemon=True)
        self._thread.start()

    def _run(self) -> None:
        while not self._stop_event.wait(self.interval_seconds):
            self.scrape("cadence")

    def scrape(self, boundary: str) -> dict[str, Any]:
        started = time.monotonic()
        with self._scrape_lock:
            snapshot = collect_phase_snapshot(self.port, self.token)
        elapsed_ms = round((time.monotonic() - started) * 1000, 3)
        with self._history_lock:
            self._sequence += 1
            self._scrape_attempts += 1
            entry = {
                "boundary": boundary,
                "sequence": self._sequence,
                "elapsed_ms": elapsed_ms,
                "snapshot": snapshot,
            }
            schema = snapshot.get("schema", {})
            if schema.get("status") == "supported":
                self._scrape_successes += 1
            else:
                reason = str(schema.get("reason") or "metrics_unsupported")
                self._scrape_failures += 1
                self._scrape_failure_reasons[reason] = (
                    self._scrape_failure_reasons.get(reason, 0) + 1
                )
                self._record_error_locked(reason, boundary)
            if len(self._snapshots) >= MAX_METRICS_SCRAPES:
                self._snapshots.pop(0)
                self._history_truncated_count += 1
            self._snapshots.append(entry)
        return snapshot

    def _record_error_locked(self, reason: str, boundary: str) -> None:
        if len(self._errors) < MAX_METRICS_SCRAPE_ERRORS:
            self._errors.append({"boundary": boundary, "reason": reason})

    def stop(self) -> None:
        self._stop_event.set()
        if self._thread is not None:
            self._thread.join(timeout=5)
            if self._thread.is_alive():
                with self._history_lock:
                    self._record_error_locked("collector_join_timeout", "collector")

    def evidence(self) -> dict[str, Any]:
        with self._history_lock:
            retained_sequences = [entry["sequence"] for entry in self._snapshots]
            return {
                "interval_seconds": self.interval_seconds,
                "cadence_mode": (
                    "bounded_post_scrape" if self.interval_seconds > 0 else "boundary_only"
                ),
                "max_snapshots": MAX_METRICS_SCRAPES,
                "scrapes": list(self._snapshots),
                "errors": list(self._errors),
                "scrape_attempts": self._scrape_attempts,
                "scrape_successes": self._scrape_successes,
                "scrape_failures": self._scrape_failures,
                "scrape_failure_reasons": dict(sorted(self._scrape_failure_reasons.items())),
                "history_retained": len(self._snapshots),
                "history_truncated": self._history_truncated_count > 0,
                "history_truncated_count": self._history_truncated_count,
                "history_sequence_range": (
                    [retained_sequences[0], retained_sequences[-1]]
                    if retained_sequences
                    else None
                ),
                "joined": self._thread is None or not self._thread.is_alive(),
            }


def collect_phase_metrics(port: int, token: str) -> dict[str, dict[str, float]]:
    """Compatibility view for callers that only need selected phase values."""

    return collect_phase_snapshot(port, token)["values"]


def phase_metric_matches(labels: dict[str, str]) -> bool:
    """Match either the legacy or operation label shape for compatibility tests."""

    for key, value in PHASE_METRIC_SELECTOR.items():
        if key == "operation" and key not in labels:
            continue
        if labels.get(key) != value:
            return False
    return True


def phase_metric_matches_schema(labels: dict[str, str], schema_kind: str) -> bool:
    selector = (
        PHASE_METRIC_SELECTOR
        if schema_kind == PHASE_SCHEMA_OPERATION
        else LEGACY_PHASE_METRIC_SELECTOR
    )
    return all(labels.get(key) == value for key, value in selector.items())


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
    connection: http.client.HTTPConnection | None = None
    try:
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        connection.request(method, path, body=body, headers=headers)
        response = connection.getresponse()
        response_body = response.read()
        response_headers = {
            name.lower(): value for name, value in response.getheaders()
        }
        recorder.add_status(
            lane,
            response.status,
            (time.monotonic() - started) * 1000,
            response_body,
            response_headers,
        )
    except (OSError, http.client.HTTPException, TimeoutError) as error:
        recorder.add_error(lane, error)
    finally:
        if connection is not None:
            connection.close()


def synthetic_credentials(labels: tuple[str, ...]) -> tuple[Credential, ...]:
    return tuple(
        Credential(label=label, secret=label, token_id=None, billing_subject=None)
        for label in labels
    )


def bound_fixture_account_ids(labels: tuple[str, ...]) -> tuple[str, ...]:
    if len(labels) < REQUIRED_IDENTITY_COUNT:
        raise ValueError(
            f"bound fixture requires {REQUIRED_IDENTITY_COUNT} token labels"
        )
    return tuple(
        f"{BOUND_FIXTURE_ACCOUNT_PREFIX}{index:02d}"
        for index in range(1, REQUIRED_IDENTITY_COUNT + 1)
    )


def scenario_bindings(
    name: str,
    credentials: tuple[Credential, ...],
    subjects: tuple[str, ...],
) -> tuple[tuple[Credential, ...], tuple[str, ...]]:
    try:
        indices = SCENARIO_IDENTITY_INDICES[name]
    except KeyError as error:
        raise ValueError(f"unknown workload scenario: {name}") from error
    try:
        return (
            tuple(credentials[index] for index in indices["credentials"]),
            tuple(subjects[index] for index in indices["subjects"]),
        )
    except IndexError as error:
        raise ValueError(
            f"scenario {name} requires {REQUIRED_IDENTITY_COUNT} credential and subject slots"
        ) from error


def run_scenario_window(
    port: int,
    deadline: float,
    scenario: dict[str, Any],
    credentials: tuple[Credential, ...],
    subjects: tuple[str, ...],
    interval_seconds: float,
    recorder: SampleRecorder,
    stop_event: threading.Event,
) -> None:
    payload = b'{"query":"synthetic","search_depth":"basic","max_results":1}'
    worker_count = int(scenario["concurrency"])
    barrier = threading.Barrier(worker_count)
    bound_credentials, bound_subjects = scenario_bindings(scenario["name"], credentials, subjects)

    def worker(slot: int) -> None:
        try:
            barrier.wait(timeout=10)
            next_request = time.monotonic()
            while not stop_event.is_set() and time.monotonic() < deadline:
                credential = bound_credentials[slot % len(bound_credentials)]
                subject = bound_subjects[slot % len(bound_subjects)]
                request_once(
                    port,
                    "/api/tavily/search",
                    payload,
                    {
                        "Content-Type": "application/json",
                        "Authorization": f"Bearer {credential.secret}",
                        "X-Perf-Subject": subject,
                    },
                    recorder,
                    "business",
                )
                next_request += interval_seconds
                if stop_event.wait(max(0, next_request - time.monotonic())):
                    return
        except Exception as error:
            recorder.add_error("scenario", error)

    workers = [
        threading.Thread(target=worker, args=(slot,), daemon=True)
        for slot in range(worker_count)
    ]
    for thread in workers:
        thread.start()
    for thread in workers:
        thread.join()


def run_control_window(
    port: int,
    deadline: float,
    interval_seconds: float,
    recorder: SampleRecorder,
    stop_event: threading.Event,
) -> None:
    next_request = time.monotonic()
    while not stop_event.is_set() and time.monotonic() < deadline:
        request_once(
            port,
            "/health",
            b"{}",
            {"Content-Type": "application/json"},
            recorder,
            "unrelated",
            method="GET",
        )
        next_request += interval_seconds
        if stop_event.wait(max(0, next_request - time.monotonic())):
            return


def run_workload_window(
    port: int,
    duration_seconds: int,
    credentials: tuple[Credential, ...],
    subjects: tuple[str, ...],
    interval_seconds: float,
) -> tuple[SampleRecorder, list[dict[str, Any]]]:
    scenarios = scenario_contract(interval_seconds)
    scenario_recorders = {scenario["name"]: SampleRecorder() for scenario in scenarios}
    control_recorder = SampleRecorder()
    stop_event = threading.Event()
    deadline = time.monotonic() + duration_seconds
    threads = [
        threading.Thread(
            target=run_scenario_window,
            args=(
                port,
                deadline,
                scenario,
                credentials,
                subjects,
                interval_seconds,
                scenario_recorders[scenario["name"]],
                stop_event,
            ),
            daemon=True,
        )
        for scenario in scenarios
    ]
    control_thread = threading.Thread(
        target=run_control_window,
        args=(
            port,
            deadline,
            DEFAULT_CONTROL_INTERVAL_SECONDS,
            control_recorder,
            stop_event,
        ),
        daemon=True,
    )
    try:
        for thread in threads:
            thread.start()
        control_thread.start()
        for thread in threads:
            thread.join()
        control_thread.join()
    finally:
        stop_event.set()
        for thread in threads:
            thread.join(timeout=2)
        control_thread.join(timeout=2)

    aggregate = SampleRecorder()
    scenarios_result: list[dict[str, Any]] = []
    for scenario in scenarios:
        recorder = scenario_recorders[scenario["name"]]
        aggregate.merge(recorder)
        bound_credentials, bound_subjects = scenario_bindings(
            scenario["name"], credentials, subjects
        )
        recorded = recorder.result(duration_seconds)
        scenarios_result.append(
            {
                "name": scenario["name"],
                "driver": scenario["driver"],
                "concurrency": scenario["concurrency"],
                "credential_mode": scenario["credential_mode"],
                "credential_labels": [credential.label for credential in bound_credentials],
                "credential_ids": [credential.token_id for credential in bound_credentials],
                "billing_subjects": [
                    credential.billing_subject for credential in bound_credentials
                ],
                "subjects": list(bound_subjects),
                "scheduling": scenario["scheduling"],
                "expected": {
                    "outcome": scenario["expected_outcome"],
                    "status_class": scenario["expected_status_class"],
                },
                "actual": {
                    "outcomes": recorded["business_outcomes"],
                    "status_counts": recorded["business_statuses"],
                    "success_request": recorded["success_request"],
                },
                "request": recorded["request"],
                "success_request": recorded["success_request"],
                "statuses": recorded["statuses"],
                "outcome_counts": recorded["outcome_counts"],
                "business_statuses": recorded["business_statuses"],
                "response_evidence": recorded["response_evidence"],
                "errors": recorded["errors"],
                "accepted_progress": recorded["accepted_progress"],
            }
        )
    aggregate.merge(control_recorder)
    return aggregate, scenarios_result


def run_load(
    port: int,
    warmup_seconds: int,
    duration_seconds: int,
    subjects: tuple[str, ...],
    credentials: tuple[Credential, ...],
    interval_seconds: float,
    on_measurement_start: Callable[[], dict[str, Any]] | None = None,
) -> tuple[dict[str, Any], dict[str, Any]]:
    warmup_observation: dict[str, Any] | None = None
    if warmup_seconds > 0:
        warmup_recorder, warmup_scenarios = run_workload_window(
            port, warmup_seconds, credentials, subjects, interval_seconds
        )
        warmup_observation = warmup_recorder.result(warmup_seconds)
        warmup_observation["scenarios"] = warmup_scenarios
    phase_baseline = on_measurement_start() if on_measurement_start else {}
    recorder, scenarios_result = run_workload_window(
        port, duration_seconds, credentials, subjects, interval_seconds
    )
    result = recorder.result(duration_seconds)
    result["scenarios"] = scenarios_result
    result["scenario_contract"] = scenario_contract(interval_seconds)
    result["credential_mapping"] = credential_mapping_evidence(
        credentials, interval_seconds
    )
    result["scenario_digest"] = scenario_digest(credentials, interval_seconds)
    result["fixture_settings"] = dict(SYNTHETIC_FIXTURE_SETTINGS)
    result["warmup"] = warmup_observation
    return result, phase_baseline


def retry_after_seconds(response: http.client.HTTPResponse) -> float | None:
    raw_value = response.headers.get("Retry-After")
    if raw_value is None:
        return None
    try:
        value = float(raw_value)
    except ValueError:
        return None
    return value if value >= 0 else None


def bootstrap_business_tokens(
    port: int, labels: tuple[str, ...]
) -> tuple[tuple[Credential, ...], dict[str, Any]]:
    started = time.monotonic()
    credentials: list[Credential] = []
    evidence: dict[str, Any] = {
        "status": "pending",
        "retry_budget_seconds": TOKEN_BOOTSTRAP_RETRY_BUDGET_SECONDS,
        "max_retries_per_label": TOKEN_BOOTSTRAP_MAX_RETRIES_PER_LABEL,
        "retry_policy": {
            "status": 503,
            "requires_retry_after": True,
            "basis": "server_deferred_write",
        },
        "attempts": [],
        "completed_labels": [],
        "remaining_labels": list(labels),
    }

    def finish(
        status: str, reason: str | None = None
    ) -> tuple[tuple[Credential, ...], dict[str, Any]]:
        evidence["status"] = status
        if reason is not None:
            evidence["reason"] = reason
        evidence["completed_labels"] = [credential.label for credential in credentials]
        evidence["remaining_labels"] = [
            label for label in labels if label not in evidence["completed_labels"]
        ]
        evidence["elapsed_ms"] = round((time.monotonic() - started) * 1000, 3)
        return tuple(credentials), evidence

    for label in labels:
        retry_count = 0
        while True:
            elapsed = time.monotonic() - started
            if elapsed >= TOKEN_BOOTSTRAP_RETRY_BUDGET_SECONDS:
                return finish("failed", "token_bootstrap_retry_budget_exhausted")
            attempt_started = time.monotonic()
            payload = json.dumps({"note": f"perf-local-{label}"}).encode("utf-8")
            connection: http.client.HTTPConnection | None = None
            try:
                connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
                connection.request(
                    "POST",
                    "/api/tokens",
                    body=payload,
                    headers={"Content-Type": "application/json"},
                )
                response = connection.getresponse()
                body = response.read()
            except (OSError, http.client.HTTPException, TimeoutError) as error:
                evidence["attempts"].append(
                    {
                        "label": label,
                        "attempt": retry_count + 1,
                        "status": None,
                        "reason": "token_bootstrap_transport",
                        "error_type": type(error).__name__,
                        "elapsed_ms": round((time.monotonic() - attempt_started) * 1000, 3),
                        "decision": "fail",
                    }
                )
                return finish("failed", "token_bootstrap_transport")
            finally:
                if connection is not None:
                    connection.close()

            retry_after = retry_after_seconds(response)
            retryable = response.status == 503 and retry_after is not None
            attempt = {
                "label": label,
                "attempt": retry_count + 1,
                "status": response.status,
                "retry_after_seconds": retry_after,
                "retryable": retryable,
                "retry_basis": "http_503_retry_after" if retryable else None,
                "elapsed_ms": round((time.monotonic() - attempt_started) * 1000, 3),
            }
            if response.status != 201:
                if not retryable:
                    attempt["decision"] = "fail"
                    evidence["attempts"].append(attempt)
                    return finish("failed", f"token_bootstrap_status_{response.status}")
                remaining_budget = TOKEN_BOOTSTRAP_RETRY_BUDGET_SECONDS - (
                    time.monotonic() - started
                )
                if retry_count >= TOKEN_BOOTSTRAP_MAX_RETRIES_PER_LABEL or remaining_budget <= 0:
                    attempt["decision"] = "fail"
                    evidence["attempts"].append(attempt)
                    return finish("failed", "token_bootstrap_retry_budget_exhausted")
                sleep_seconds = min(retry_after, remaining_budget)
                attempt["decision"] = "retry"
                attempt["sleep_seconds"] = sleep_seconds
                evidence["attempts"].append(attempt)
                retry_count += 1
                if sleep_seconds > 0:
                    time.sleep(sleep_seconds)
                continue

            attempt["decision"] = "accept"
            evidence["attempts"].append(attempt)
            try:
                token = json.loads(body).get("token")
            except (ValueError, UnicodeDecodeError):
                return finish("failed", "token_bootstrap_invalid_json")
            if not isinstance(token, str) or not token:
                return finish("failed", "token_bootstrap_missing_secret")
            token_parts = token.split("-", 2)
            if len(token_parts) != 3 or token_parts[0] != "th" or not token_parts[1]:
                return finish("failed", "token_bootstrap_invalid_identity")
            token_id = token_parts[1]
            credentials.append(
                Credential(
                    label=label,
                    secret=token,
                    token_id=token_id,
                    billing_subject=f"token:{token_id}",
                )
            )
            break
    return finish("ready")


def verify_credential_bindings(
    database_path: Path, credentials: tuple[Credential, ...]
) -> dict[str, Any]:
    token_ids = [credential.token_id for credential in credentials if credential.token_id]
    if len(token_ids) != len(credentials):
        return {"status": "failed", "reason": "missing_token_id"}
    placeholders = ",".join("?" for _ in token_ids)
    try:
        with closing(sqlite3.connect(database_path, timeout=5)) as connection:
            rows = connection.execute(
                f"SELECT t.id, b.user_id "
                f"FROM auth_tokens t LEFT JOIN user_token_bindings b ON b.token_id = t.id "
                f"WHERE t.id IN ({placeholders})",
                token_ids,
            ).fetchall()
    except sqlite3.Error as error:
        return {"status": "failed", "reason": f"database_query:{type(error).__name__}"}
    observed = {token_id: owner_id for token_id, owner_id in rows}
    missing = sorted(set(token_ids) - set(observed))
    bound = sorted(token_id for token_id, owner_id in observed.items() if owner_id is not None)
    return {
        "status": "passed" if not missing and not bound else "failed",
        "source": "auth_tokens LEFT JOIN user_token_bindings",
        "expected_binding": "unbound",
        "observed_token_ids": sorted(observed),
        "missing_token_ids": missing,
        "bound_token_ids": bound,
    }


def verify_bound_credential_bindings(
    database_path: Path, credentials: tuple[Credential, ...]
) -> dict[str, Any]:
    expected: dict[str, dict[str, Any]] = {}
    for index, credential in enumerate(credentials, start=1):
        token_id = credential.token_id
        billing_subject = credential.billing_subject
        expected_account_id = f"{BOUND_FIXTURE_ACCOUNT_PREFIX}{index:02d}"
        if not token_id or not billing_subject:
            return {"status": "failed", "reason": "missing_bound_identity"}
        if token_id in expected:
            return {"status": "failed", "reason": "duplicate_bound_token_id"}
        if billing_subject != f"account:{expected_account_id}":
            return {"status": "failed", "reason": "bound_billing_subject_mismatch"}
        expected[token_id] = {
            "account_id": expected_account_id,
            "billing_subject": f"account:{expected_account_id}",
        }

    if not expected:
        return {"status": "failed", "reason": "empty_bound_identity"}

    placeholders = ",".join("?" for _ in expected)
    try:
        with closing(sqlite3.connect(database_path, timeout=10)) as connection:
            connection.execute("PRAGMA busy_timeout = 10000")
            rows = connection.execute(
                f"""
                SELECT t.id,
                       b.user_id,
                       q.business_calls_1h_limit,
                       q.daily_credits_limit,
                       q.monthly_credits_limit,
                       q.inherits_defaults,
                       q.created_at,
                       q.updated_at
                FROM auth_tokens t
                LEFT JOIN user_token_bindings b ON b.token_id = t.id
                LEFT JOIN account_quota_limits q ON q.user_id = b.user_id
                WHERE t.id IN ({placeholders})
                """,
                list(expected),
            ).fetchall()
    except sqlite3.Error as error:
        return {"status": "failed", "reason": f"database_query:{type(error).__name__}"}

    observed: dict[str, dict[str, Any]] = {}
    mismatches: list[dict[str, Any]] = []
    for token_id, user_id, hourly, daily, monthly, inherits_defaults, created_at, updated_at in rows:
        observed[token_id] = {
            "token_id": token_id,
            "user_id": user_id,
            "billing_subject": f"account:{user_id}" if user_id else None,
            "quota_headroom": {
                "business_calls_1h_limit": hourly,
                "daily_credits_limit": daily,
                "monthly_credits_limit": monthly,
            },
            "inherits_defaults": inherits_defaults,
            "quota_created_at": created_at,
            "quota_updated_at": updated_at,
        }
        expected_identity = expected.get(token_id)
        if expected_identity is None:
            mismatches.append({"token_id": token_id, "reason": "unexpected_token_id"})
            continue
        observed_identity = observed[token_id]
        if (
            observed_identity["user_id"] != expected_identity["account_id"]
            or observed_identity["billing_subject"] != expected_identity["billing_subject"]
            or observed_identity["quota_headroom"] != BOUND_FIXTURE_QUOTA_HEADROOM
            or observed_identity["inherits_defaults"] != 0
            or not isinstance(observed_identity["quota_created_at"], int)
            or observed_identity["quota_created_at"] <= 0
            or not isinstance(observed_identity["quota_updated_at"], int)
            or observed_identity["quota_updated_at"] <= 0
        ):
            mismatches.append(
                {
                    "token_id": token_id,
                    "expected": {
                        "user_id": expected_identity["account_id"],
                        "billing_subject": expected_identity["billing_subject"],
                        "quota_headroom": dict(BOUND_FIXTURE_QUOTA_HEADROOM),
                        "inherits_defaults": 0,
                        "quota_timestamp": "positive_unix_seconds",
                    },
                    "observed": observed_identity,
                }
            )

    missing = sorted(set(expected) - set(observed))
    return {
        "status": "passed" if not missing and not mismatches else "failed",
        "source": BOUND_FIXTURE_SOURCE,
        "expected_binding": "bound_account",
        "observed": [observed[token_id] for token_id in sorted(observed)],
        "missing_token_ids": missing,
        "mismatches": mismatches,
    }


def seed_bound_account_users(
    database_path: Path, account_ids: tuple[str, ...]
) -> dict[str, Any]:
    now = int(time.time())
    try:
        with closing(sqlite3.connect(database_path, timeout=10)) as connection:
            with connection:
                connection.execute("PRAGMA busy_timeout = 10000")
                connection.execute("PRAGMA foreign_keys = ON")
                for index, account_id in enumerate(account_ids, start=1):
                    connection.execute(
                    """
                    INSERT INTO users (
                        id, display_name, username, avatar_template, active,
                        debug_info_shared, created_at, updated_at, last_login_at
                    ) VALUES (?, ?, ?, NULL, 1, 0, ?, ?, NULL)
                    ON CONFLICT(id) DO UPDATE SET
                        display_name = excluded.display_name,
                        username = excluded.username,
                        active = 1,
                        debug_info_shared = 0,
                        updated_at = excluded.updated_at,
                        last_login_at = NULL
                    """,
                        (
                            account_id,
                            f"Performance account {index:02d}",
                            f"perf-account-{index:02d}",
                            now,
                            now,
                        ),
                    )
                    connection.execute(
                    """
                    INSERT INTO account_quota_limits (
                        user_id,
                        business_calls_1h_limit,
                        daily_credits_limit,
                        monthly_credits_limit,
                        monthly_broken_limit,
                        monthly_blocked_key_limit_delta,
                        inherits_defaults,
                        created_at,
                        updated_at
                    ) VALUES (?, ?, ?, ?, 5, 0, 0, ?, ?)
                    ON CONFLICT(user_id) DO UPDATE SET
                        business_calls_1h_limit = excluded.business_calls_1h_limit,
                        daily_credits_limit = excluded.daily_credits_limit,
                        monthly_credits_limit = excluded.monthly_credits_limit,
                        monthly_broken_limit = excluded.monthly_broken_limit,
                        monthly_blocked_key_limit_delta = excluded.monthly_blocked_key_limit_delta,
                        inherits_defaults = 0,
                        updated_at = excluded.updated_at
                    """,
                        (
                            account_id,
                            BOUND_FIXTURE_QUOTA_HEADROOM["business_calls_1h_limit"],
                            BOUND_FIXTURE_QUOTA_HEADROOM["daily_credits_limit"],
                            BOUND_FIXTURE_QUOTA_HEADROOM["monthly_credits_limit"],
                            now,
                            now,
                        ),
                    )
    except sqlite3.Error as error:
        return {
            "status": "failed",
            "reason": f"database_seed:{type(error).__name__}",
            "source": BOUND_FIXTURE_SOURCE,
            "elapsed_ms": None,
        }
    return {
        "status": "ready",
        "source": "SQLite users + account_quota_limits synthetic account seed",
        "account_ids": list(account_ids),
        "quota_headroom": dict(BOUND_FIXTURE_QUOTA_HEADROOM),
        "inherits_defaults": 0,
        "seeded_at_unix": now,
    }


def create_bound_token_via_admin(
    port: int, account_id: str
) -> tuple[Credential | None, dict[str, Any]]:
    connection: http.client.HTTPConnection | None = None
    try:
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        connection.request(
            "POST",
            f"/api/users/{account_id}/tokens",
            body=b"",
            headers={"Content-Length": "0"},
        )
        response = connection.getresponse()
        body = response.read()
    except (OSError, http.client.HTTPException, TimeoutError) as error:
        return None, {
            "account_id": account_id,
            "status": None,
            "decision": "fail",
            "reason": "management_interface_transport",
            "error_type": type(error).__name__,
        }
    finally:
        if connection is not None:
            connection.close()

    try:
        token = json.loads(body).get("token")
    except (ValueError, UnicodeDecodeError, AttributeError):
        token = None
    retry_after = retry_after_seconds(response)
    database_locked = b"database is locked" in body.lower()
    retryable = database_locked or (response.status == 503 and retry_after is not None)
    if response.status != 201 or not isinstance(token, str) or not token:
        return None, {
            "account_id": account_id,
            "status": response.status,
            "decision": "fail",
            "reason": "management_interface_status_or_token_invalid",
            "retryable": retryable,
            "retry_after_seconds": retry_after,
            "retry_basis": (
                "sqlite_database_locked"
                if database_locked
                else "http_503_retry_after"
                if retryable
                else None
            ),
        }
    parts = token.split("-", 2)
    if (
        len(parts) != 3
        or parts[0] != "th"
        or len(parts[1]) != 4
        or not parts[1].isalnum()
        or len(parts[2]) not in {12, 24}
        or not parts[2].isalnum()
    ):
        return None, {
            "account_id": account_id,
            "status": response.status,
            "decision": "fail",
            "reason": "management_interface_token_identity_invalid",
            "retryable": False,
            "retry_after_seconds": retry_after,
            "retry_basis": None,
        }
    return (
        Credential(
            label=account_id,
            secret=token,
            token_id=parts[1],
            billing_subject=f"account:{account_id}",
        ),
        {
            "account_id": account_id,
            "status": response.status,
            "token_id": parts[1],
            "decision": "accept",
            "secret_persisted": False,
        },
    )


def bootstrap_bound_business_accounts(
    port: int,
    database_path: Path,
    labels: tuple[str, ...],
    interval_seconds: float = DEFAULT_REQUEST_INTERVAL_SECONDS,
) -> tuple[tuple[Credential, ...], dict[str, Any]]:
    started = time.monotonic()
    try:
        account_ids = bound_fixture_account_ids(labels)
    except ValueError as error:
        return (), {
            "status": "failed",
            "reason": str(error),
            "source": BOUND_FIXTURE_SOURCE,
        }

    user_seed = seed_bound_account_users(database_path, account_ids)
    if user_seed["status"] != "ready":
        user_seed["elapsed_ms"] = round((time.monotonic() - started) * 1000, 3)
        return (), user_seed

    credentials: list[Credential] = []
    management_attempts: list[dict[str, Any]] = []
    for label, account_id in zip(labels[:REQUIRED_IDENTITY_COUNT], account_ids, strict=True):
        retry_count = 0
        while True:
            credential, attempt = create_bound_token_via_admin(port, account_id)
            attempt_record = {**attempt, "label": label}
            management_attempts.append(attempt_record)
            if credential is not None:
                break

            remaining_budget = TOKEN_BOOTSTRAP_RETRY_BUDGET_SECONDS - (
                time.monotonic() - started
            )
            if (
                not attempt.get("retryable")
                or retry_count >= TOKEN_BOOTSTRAP_MAX_RETRIES_PER_LABEL
                or remaining_budget <= 0
            ):
                return (), {
                    "status": "failed",
                    "reason": attempt.get("reason", "management_interface_binding_failed"),
                    "source": BOUND_FIXTURE_SOURCE,
                    "binding_path": "POST /api/users/<account-id>/tokens",
                    "user_seed": user_seed,
                    "management_attempts": management_attempts,
                    "elapsed_ms": round((time.monotonic() - started) * 1000, 3),
                }

            retry_after = attempt.get("retry_after_seconds")
            sleep_seconds = (
                retry_after
                if isinstance(retry_after, (int, float)) and retry_after >= 0
                else BOUND_TOKEN_BOOTSTRAP_RETRY_DELAY_SECONDS
            )
            sleep_seconds = min(float(sleep_seconds), remaining_budget)
            attempt_record["decision"] = "retry"
            attempt_record["sleep_seconds"] = sleep_seconds
            retry_count += 1
            if sleep_seconds > 0:
                time.sleep(sleep_seconds)
        credentials.append(
            Credential(
                label=label,
                secret=credential.secret,
                token_id=credential.token_id,
                billing_subject=credential.billing_subject,
            )
        )

    actual_credentials = tuple(credentials)
    runtime_binding_check = verify_bound_credential_bindings(
        database_path, actual_credentials
    )
    mapping = credential_mapping_evidence(actual_credentials, interval_seconds)
    evidence = {
        "status": "ready" if runtime_binding_check["status"] == "passed" else "failed",
        "source": BOUND_FIXTURE_SOURCE,
        "binding_path": "POST /api/users/<account-id>/tokens",
        "binding_kind": "bound_account",
        "billing_subject_kind": "account",
        "account_ids": list(account_ids),
        "quota_headroom": dict(BOUND_FIXTURE_QUOTA_HEADROOM),
        "credentials": mapping["credentials"],
        "scenario_digest": mapping["scenario_digest"],
        "secret_persisted": False,
        "user_seed": user_seed,
        "management_attempts": management_attempts,
        "runtime_binding_check": runtime_binding_check,
        "elapsed_ms": round((time.monotonic() - started) * 1000, 3),
    }
    if evidence["status"] != "ready":
        evidence["reason"] = "bound_fixture_verification_failed"
    return actual_credentials, evidence


def run_bound_fixture_precheck(
    port: int,
    database_path: Path,
    credential: Credential,
    subject: str,
) -> dict[str, Any]:
    if not credential.token_id or not credential.billing_subject:
        return {"status": "failed", "reason": "bound_identity_missing"}
    payload = b'{"query":"synthetic","search_depth":"basic","max_results":1}'
    connection: http.client.HTTPConnection | None = None
    try:
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        connection.request(
            "POST",
            "/api/tavily/search",
            body=payload,
            headers={
                "Content-Type": "application/json",
                "Authorization": f"Bearer {credential.secret}",
                "X-Perf-Subject": subject,
            },
        )
        response = connection.getresponse()
        body = response.read()
    except (OSError, http.client.HTTPException, TimeoutError) as error:
        return {
            "status": "failed",
            "reason": "transport",
            "error_type": type(error).__name__,
        }
    finally:
        if connection is not None:
            connection.close()

    try:
        decoded = json.loads(body)
    except (ValueError, UnicodeDecodeError):
        decoded = None
    usage_credits = (
        decoded.get("usage", {}).get("credits")
        if isinstance(decoded, dict) and isinstance(decoded.get("usage"), dict)
        else None
    )
    billing_observation: dict[str, Any] | None = None
    try:
        with closing(sqlite3.connect(database_path, timeout=10)) as database:
            database.execute("PRAGMA busy_timeout = 10000")
            row = database.execute(
                """
                SELECT billing_subject, business_credits, result_status, billing_state
                FROM auth_token_logs
                WHERE token_id = ?
                ORDER BY id DESC
                LIMIT 1
                """,
                (credential.token_id,),
            ).fetchone()
            if row is not None:
                billing_observation = {
                    "billing_subject": row[0],
                    "business_credits": row[1],
                    "result_status": row[2],
                    "billing_state": row[3],
                }
    except sqlite3.Error as error:
        return {
            "status": "failed",
            "reason": f"billing_query:{type(error).__name__}",
            "http_status": response.status,
            "response_usage_credits": usage_credits,
        }

    passed = (
        response.status == 200
        and usage_credits == 1
        and billing_observation is not None
        and billing_observation["billing_subject"] == credential.billing_subject
        and billing_observation["business_credits"] == 1
        and billing_observation["result_status"] == "success"
    )
    return {
        "status": "passed" if passed else "failed",
        "reason": None if passed else "bound_request_or_billing_mismatch",
        "upstream_kind": "loopback_synthetic_mock",
        "http_status": response.status,
        "response_usage_credits": usage_credits,
        "billing_subject": credential.billing_subject,
        "billing_observation": billing_observation,
        "secret_persisted": False,
    }


def phase_delta(
    before: dict[str, Any], after: dict[str, Any]
) -> dict[str, dict[str, float]]:
    before_values = before.get("values", before)
    after_values = after.get("values", after)
    phases: dict[str, dict[str, float]] = {}
    for phase in sorted(set(before_values) | set(after_values)):
        current = after_values.get(phase, {})
        initial = before_values.get(phase, {})
        phases[phase] = {
            "count": max(0.0, current.get("count", 0.0) - initial.get("count", 0.0)),
            "sum_seconds": max(
                0.0,
                current.get("sum_seconds", 0.0) - initial.get("sum_seconds", 0.0),
            ),
        }
    return phases


def counter_delta(before: dict[str, float], after: dict[str, float]) -> dict[str, float]:
    return {
        key: max(0.0, float(after.get(key, 0.0)) - float(before.get(key, 0.0)))
        for key in sorted(set(before) | set(after))
        if float(after.get(key, 0.0)) >= float(before.get(key, 0.0))
    }


def observability_delta(
    before: dict[str, Any], after: dict[str, Any]
) -> dict[str, dict[str, float]]:
    before_values = before.get("observability", {})
    after_values = after.get("observability", {})
    return {
        family: counter_delta(
            before_values.get(family, {}), after_values.get(family, {})
        )
        for family in ("dropped", "flush")
    }


def phase_schema_delta(
    before: dict[str, Any], after: dict[str, Any], phases: dict[str, dict[str, float]]
) -> dict[str, Any]:
    before_schema = before.get("schema", {})
    after_schema = after.get("schema", {})
    before_kind = before_schema.get("kind")
    after_kind = after_schema.get("kind")
    if after_schema.get("status") != "supported":
        return {
            **after_schema,
            "status": "unsupported",
            "before_kind": before_kind,
            "after_kind": after_kind,
            "coverage_count": 0.0,
            "coverage_phase": None,
        }
    if before_kind != after_kind:
        return {
            "status": "unsupported",
            "kind": PHASE_SCHEMA_UNSUPPORTED,
            "reason": "phase_schema_changed_during_measurement",
            "before_kind": before_kind,
            "after_kind": after_kind,
            "metric_name": after_schema.get("metric_name"),
            "selector": after_schema.get("selector"),
            "phase_keys": sorted(phases),
            "coverage_count": 0.0,
            "coverage_phase": None,
        }
    coverage_phase = "total" if "total" in phases else None
    if coverage_phase is None and phases:
        coverage_phase = max(
            phases,
            key=lambda phase: float(phases[phase].get("count", 0.0)),
        )
    coverage_count = (
        float(phases[coverage_phase].get("count", 0.0))
        if coverage_phase is not None
        else 0.0
    )
    return {
        **after_schema,
        "before_kind": before_kind,
        "after_kind": after_kind,
        "coverage_count": coverage_count,
        "coverage_phase": coverage_phase,
    }


def parse_memory_bytes(value: str) -> int:
    match = re.fullmatch(r"(?i)([0-9]+)(b|kib|mib|gib|kb|mb|gb)?", value.strip())
    if match is None:
        raise ValueError(f"invalid memory resource: {value}")
    amount = int(match.group(1))
    unit = (match.group(2) or "b").lower()
    multiplier = {
        "b": 1,
        "kb": 1000,
        "mb": 1000**2,
        "gb": 1000**3,
        "kib": 1024,
        "mib": 1024**2,
        "gib": 1024**3,
    }[unit]
    return amount * multiplier


def parse_resource_spec(raw: str) -> dict[str, Any]:
    values: dict[str, str] = {}
    for item in raw.split(","):
        key, separator, value = item.partition("=")
        if not separator or not key or not value:
            raise ValueError(f"invalid resource item: {item}")
        values[key.strip()] = value.strip()
    try:
        cpu = int(values["cpu"])
        memory_bytes = parse_memory_bytes(values["memory"])
        sqlite_pool = int(values["sqlite_pool"])
    except (KeyError, ValueError) as error:
        raise ValueError("resources require cpu, memory and sqlite_pool") from error
    if cpu < 1 or memory_bytes < 1 or sqlite_pool < 1:
        raise ValueError("resource limits must be positive")
    return {
        "raw": raw,
        "cpu": cpu,
        "memory": values["memory"],
        "memory_bytes": memory_bytes,
        "sqlite_pool": sqlite_pool,
    }


def host_memory_bytes() -> int | None:
    try:
        for line in Path("/proc/meminfo").read_text(encoding="ascii").splitlines():
            if line.startswith("MemTotal:"):
                return int(line.split()[1]) * 1024
    except (FileNotFoundError, OSError, ValueError):
        return None
    return None


def cgroup_observation(pid: int) -> dict[str, Any]:
    result: dict[str, Any] = {"path": None, "memory_max_bytes": None, "memory_max_raw": None}
    try:
        lines = Path(f"/proc/{pid}/cgroup").read_text(encoding="ascii").splitlines()
    except (FileNotFoundError, OSError):
        return result
    for line in lines:
        hierarchy, _controllers, path = line.split(":", 2)
        if hierarchy == "0":
            result["path"] = path
            candidate = Path("/sys/fs/cgroup") / path.lstrip("/") / "memory.max"
            try:
                raw = candidate.read_text(encoding="ascii").strip()
            except (FileNotFoundError, OSError):
                raw = None
            result["memory_max_raw"] = raw
            if raw and raw != "max":
                try:
                    result["memory_max_bytes"] = int(raw)
                except ValueError:
                    pass
            break
    return result


def source_sqlite_pool_limit(repo: Path) -> int | None:
    source_path = repo / "src" / "lib.rs"
    try:
        source = source_path.read_text(encoding="utf-8")
    except (FileNotFoundError, OSError):
        return None
    match = re.search(
        r"pub\s+const\s+SQLITE_POOL_MAX_CONNECTIONS_DEFAULT\s*:\s*u32\s*=\s*(\d+)",
        source,
    )
    return int(match.group(1)) if match else None


def apply_resource_constraints(
    process: subprocess.Popen[bytes], repo: Path, resource_spec: dict[str, Any]
) -> dict[str, Any]:
    requested_cpu = int(resource_spec["cpu"])
    requested_memory = int(resource_spec["memory_bytes"])
    errors: list[str] = []
    host_cpu_count = os.cpu_count()
    if host_cpu_count is None or host_cpu_count < requested_cpu:
        errors.append("requested_cpu_exceeds_host")
    elif not hasattr(os, "sched_setaffinity"):
        errors.append("cpu_affinity_unavailable")
    else:
        try:
            os.sched_setaffinity(process.pid, set(range(requested_cpu)))
        except OSError as error:
            errors.append(f"cpu_affinity_apply_failed:{type(error).__name__}")

    if not hasattr(posix_resource, "prlimit"):
        errors.append("address_space_limit_unavailable")
    else:
        try:
            posix_resource.prlimit(
                process.pid,
                posix_resource.RLIMIT_AS,
                (requested_memory, requested_memory),
            )
        except (OSError, ValueError) as error:
            errors.append(f"address_space_limit_apply_failed:{type(error).__name__}")

    observed_affinity: list[int] | None = None
    if hasattr(os, "sched_getaffinity"):
        try:
            observed_affinity = sorted(os.sched_getaffinity(process.pid))
        except OSError:
            observed_affinity = None
    observed_rlimit_as: list[int] | None = None
    if hasattr(posix_resource, "prlimit"):
        try:
            observed_rlimit_as = list(posix_resource.prlimit(process.pid, posix_resource.RLIMIT_AS))
        except (OSError, ValueError):
            observed_rlimit_as = None

    observed_pool = source_sqlite_pool_limit(repo)
    if observed_pool != int(resource_spec["sqlite_pool"]):
        errors.append(
            f"sqlite_pool_requested_{resource_spec['sqlite_pool']}_observed_{observed_pool}"
        )
    status = "validated" if not errors else "mismatch"
    return {
        "status": status,
        "scope": "launcher_process_and_inherited_descendants",
        "requested": resource_spec,
        "host": {
            "cpu_count": host_cpu_count,
            "memory_total_bytes": host_memory_bytes(),
        },
        "process": {
            "launcher_pid": process.pid,
            "cpu_affinity_cpus": observed_affinity,
            "address_space_limit_bytes": observed_rlimit_as,
            "cgroup": cgroup_observation(process.pid),
        },
        "startup": {
            "sqlite_pool_max_connections": observed_pool,
            "source": "src/lib.rs:SQLITE_POOL_MAX_CONNECTIONS_DEFAULT",
        },
        "enforcement": ["sched_setaffinity", "RLIMIT_AS"],
        "errors": errors,
    }


def terminate_process(
    process: subprocess.Popen[bytes],
    output_threads: Iterable[threading.Thread] = (),
    process_group_id: int | None = None,
) -> dict[str, Any]:
    """Stop one launcher's group and prove its pipes are drained before log close."""

    threads = tuple(output_threads)
    group_id = process_group_id if process_group_id is not None else process.pid
    initial_pids = _cleanup_process_ids(process, group_id)
    signals_sent: list[str] = []
    errors: list[dict[str, Any]] = []

    def send_group_signal(signum: signal.Signals) -> None:
        current_pids = _cleanup_process_ids(process, group_id)
        if not current_pids and process.poll() is not None:
            return
        try:
            os.killpg(group_id, signum)
            signals_sent.append(signum.name)
        except ProcessLookupError:
            if _cleanup_process_ids(process, group_id):
                errors.append(
                    {
                        "stage": "process_group_signal",
                        "signal": signum.name,
                        "reason": "process_group_disappeared",
                    }
                )
        except OSError as error:
            errors.append(
                {
                    "stage": "process_group_signal",
                    "signal": signum.name,
                    **exception_evidence(error, "process_group_signal"),
                }
            )

    def wait_for_processes(timeout_seconds: float) -> list[int]:
        deadline = time.monotonic() + timeout_seconds
        remaining = _cleanup_process_ids(process, group_id)
        while remaining and time.monotonic() < deadline:
            if process.poll() is None:
                try:
                    process.wait(timeout=min(0.1, max(0.01, deadline - time.monotonic())))
                except subprocess.TimeoutExpired:
                    pass
                except OSError as error:
                    errors.append(exception_evidence(error, "process_wait"))
            remaining = _cleanup_process_ids(process, group_id)
            if remaining:
                time.sleep(0.02)
        return remaining

    send_group_signal(signal.SIGTERM)
    remaining_pids = wait_for_processes(PROCESS_CLEANUP_TERM_TIMEOUT_SECONDS)
    if remaining_pids:
        send_group_signal(signal.SIGKILL)
        remaining_pids = wait_for_processes(PROCESS_CLEANUP_KILL_TIMEOUT_SECONDS)

    thread_deadline = time.monotonic() + PROCESS_CLEANUP_KILL_TIMEOUT_SECONDS
    for thread in threads:
        thread.join(timeout=max(0.0, thread_deadline - time.monotonic()))
    output_threads_joined = all(not thread.is_alive() for thread in threads)
    remaining_pids = _cleanup_process_ids(process, group_id)
    status = "validated" if not remaining_pids and output_threads_joined else "unvalidated"
    if remaining_pids:
        errors.append(
            {
                "stage": "process_group_cleanup",
                "reason": "processes_remain",
                "remaining_pids": remaining_pids,
            }
        )
    if not output_threads_joined:
        errors.append(
            {
                "stage": "output_thread_cleanup",
                "reason": "output_threads_remain",
                "threads": [
                    {"name": thread.name, "ident": thread.ident}
                    for thread in threads
                    if thread.is_alive()
                ],
            }
        )
    return {
        "status": status,
        "launcher_pid": process.pid,
        "process_group_id": group_id,
        "initial_pids": initial_pids,
        "remaining_pids": remaining_pids,
        "signals_sent": signals_sent,
        "output_threads_joined": output_threads_joined,
        "errors": errors,
        "returncode": process.returncode,
    }


def prepare_variant(
    root: Path,
    revision: str,
    variant: str,
    workdir: Path,
    mock: MockUpstream,
    args: argparse.Namespace,
    round_number: int,
    log_directory: Path,
) -> dict[str, Any]:
    """Prepare one isolated variant and retain the exact failing stage."""

    context: dict[str, Any] = {
        "workdir": workdir,
        "variant": variant,
        "round": round_number,
        "stdout_log": None,
        "stderr_log": None,
    }
    stage = "workdir.mkdir"
    try:
        workdir.mkdir(parents=True, exist_ok=True)
        context["workspace_storage_before"] = workspace_storage_evidence(workdir)
        stage = "repo.mkdir"
        repo = workdir / f"{variant}-repo"
        repo.mkdir()
        context["repo"] = repo
        stage = "archive_revision"
        resolved = archive_revision(root, revision, repo)
        context["resolved"] = resolved
        stage = "binary_manifest_evidence"
        binary_provenance = binary_manifest_evidence(
            getattr(args, "binary_manifest", None), repo, variant, resolved
        )
        context["binary_provenance"] = binary_provenance
        stage = "resource_spec"
        resource_spec = getattr(
            args, "resource_spec", parse_resource_spec(args.resources)
        )
        context["resource_spec"] = resource_spec
        context["scenario_shape"] = scenario_contract(args.request_interval_seconds)
        context["synthetic_mapping"] = credential_mapping_evidence(
            synthetic_credentials(tuple(args.tokens)), args.request_interval_seconds
        )
        stage = "create_fixture"
        core, sidecar = create_fixture(workdir / f"{variant}-fixture")
        context["core"] = core
        context["sidecar"] = sidecar
        stage = "find_free_port"
        context["port"] = find_free_port()
        stage = "find_metrics_port"
        context["metrics_port"] = find_free_port()
        metrics_token = "perf-metrics-token"
        stage = "metrics_token.write"
        metrics_token_path = workdir / "metrics.token"
        metrics_token_path.write_text(metrics_token + "\n", encoding="ascii")
        stage = "metrics_token.chmod"
        metrics_token_path.chmod(0o600)
        context["metrics_token"] = metrics_token
        context["metrics_token_path"] = metrics_token_path
        stage = "log_directory.mkdir"
        log_directory.mkdir(parents=True, exist_ok=True)
        context["log_directory"] = log_directory
        if not args.service_command:
            context.update(
                {
                    "service_command_binding": {
                        "status": "missing",
                        "reason": "service_command_missing",
                    },
                    "early_reason": "service_command_missing",
                }
            )
            return context
        stage = "service_command_binding"
        service_command_binding = service_command_binary_binding(
            args.service_command, binary_provenance
        )
        context["service_command_binding"] = service_command_binding
        if (
            binary_provenance.get("status") != "verified"
            or service_command_binding.get("status")
            not in {"explicit_placeholder", "explicit_environment"}
        ):
            context["early_reason"] = binary_provenance.get(
                "reason",
                service_command_binding.get("reason", "binary_provenance_unavailable"),
            )
            return context

        stage = "service_environment"
        environment = dict(os.environ)
        environment.update(
            {
                "PERF_LOCAL_VARIANT": variant,
                "PERF_LOCAL_ROUND": str(round_number),
                "PERF_LOCAL_REPO": str(repo),
                "PERF_LOCAL_DB_PATH": str(core),
                "PERF_LOCAL_SIDECAR_DB_PATH": str(sidecar),
                "PERF_LOCAL_UPSTREAM_URL": mock.url,
                "PERF_LOCAL_PORT": str(context["port"]),
                "PERF_LOCAL_METRICS_PORT": str(context["metrics_port"]),
                "PERF_LOCAL_METRICS_ENABLED": "1" if args.metrics_enabled else "0",
                "PERF_LOCAL_SUBJECTS": ",".join(args.subjects),
                "PERF_LOCAL_TOKENS": ",".join(args.tokens),
                "PERF_LOCAL_RESOURCES": args.resources,
                "PERF_LOCAL_RESOURCE_SPEC": json.dumps(resource_spec, sort_keys=True),
                "PERF_LOCAL_BINARY_MANIFEST": str(binary_provenance["manifest_path"]),
                "PERF_LOCAL_BINARY_PATH": str(binary_provenance["binary_path"]),
                "PERF_LOCAL_BINARY_SHA256": str(binary_provenance["binary_sha256"]),
                "PERF_LOCAL_SOURCE_SHA": str(binary_provenance.get("source_sha") or ""),
                "PERF_LOCAL_BUILD_INPUT_SHA": str(
                    binary_provenance.get("build_input_sha") or ""
                ),
                "TAVILY_UPSTREAM": mock.url,
                "TAVILY_USAGE_BASE": mock.url,
                "PROXY_DB_PATH": str(core),
                "PROXY_BIND": "127.0.0.1",
                "PROXY_PORT": str(context["port"]),
                "OBSERVABILITY_ENABLED": "true" if args.metrics_enabled else "false",
                "OBSERVABILITY_ENVIRONMENT": "test",
                "OBSERVABILITY_INSTANCE": "local",
                "METRICS_BIND": f"127.0.0.1:{context['metrics_port']}",
                "METRICS_TOKEN_FILE": str(metrics_token_path),
                "TAVILY_API_KEYS": "tvly-synthetic-test-key",
                **SYNTHETIC_FIXTURE_ENV,
                "HA_MODE": "single",
                "DEV_OPEN_ADMIN": "true",
            }
        )
        context["environment"] = environment
        stage = "service_command.format"
        context["command"] = args.service_command.format(
            repo=shlex.quote(str(repo)),
            db=shlex.quote(str(core)),
            sidecar=shlex.quote(str(sidecar)),
            port=context["port"],
            upstream=shlex.quote(mock.url),
            variant=variant,
            binary=shlex.quote(str(binary_provenance["binary_path"])),
            manifest=shlex.quote(str(binary_provenance["manifest_path"])),
            revision=resolved,
        )
        stage = "stdout_log.open"
        context["stdout_log"] = BoundedLogWriter(log_directory / "stdout.log")
        stage = "stderr_log.open"
        context["stderr_log"] = BoundedLogWriter(log_directory / "stderr.log")
        context["workspace_storage_after_preparation"] = workspace_storage_evidence(workdir)
        return context
    except (HarnessInterrupted, KeyboardInterrupt):
        raise
    except BaseException as error:
        raise VariantPreparationError(stage, error, context) from error


def finalize_variant_result(
    result: dict[str, Any],
    *,
    workdir: Path,
    workspace_before: dict[str, Any],
    prepared: dict[str, Any] | None,
    process: subprocess.Popen[bytes] | None,
    process_group_id: int | None,
    output_threads: Iterable[threading.Thread],
    stdout_log: BoundedLogWriter | None,
    stderr_log: BoundedLogWriter | None,
    system_collector: SystemDiagnosticsCollector | None,
    binary_provenance: dict[str, Any],
    service_command_binding: dict[str, Any],
    service_process: dict[str, Any],
    preserve_workspace: bool = False,
) -> dict[str, Any]:
    """Finalize process, pipe, diagnostic, and workspace evidence for one variant."""

    if process is None:
        process_cleanup = {
            "status": "validated",
            "reason": "process_not_started",
            "remaining_pids": [],
            "output_threads_joined": True,
            "errors": [],
        }
    else:
        process_cleanup = terminate_process(
            process, output_threads, process_group_id
        )
    if process_cleanup["status"] == "validated":
        log_cleanup = close_bounded_logs(stdout_log, stderr_log)
    else:
        log_cleanup = {
            "status": "unvalidated",
            "closed": False,
            "reason": "process_cleanup_unvalidated",
            "errors": [],
        }
    system_evidence = (
        system_collector.evidence()
        if system_collector is not None
        else unavailable_system_diagnostics("system_collector_not_started")
    )
    cleanup_status = (
        "validated"
        if process_cleanup["status"] == "validated"
        and log_cleanup["status"] == "validated"
        and system_evidence.get("joined", True)
        else "unvalidated"
    )
    cleanup = {
        "status": cleanup_status,
        "process": process_cleanup,
        "logs": log_cleanup,
        "errors": (
            process_cleanup.get("errors", [])
            + log_cleanup.get("errors", [])
            + (
                [{"stage": "system_diagnostics", "reason": "thread_remains"}]
                if not system_evidence.get("joined", True)
                else []
            )
        ),
    }
    if cleanup_status != "validated":
        result["status"] = "unvalidated"
        result["reason"] = "process_cleanup_unvalidated"
    result["cleanup"] = cleanup
    result["binary_provenance"] = binary_provenance
    result["service_command_binding"] = service_command_binding
    result["service_process"] = service_process
    result["system_diagnostics"] = system_evidence
    result["logs"] = {
        "stdout": stdout_log.evidence() if stdout_log is not None else None,
        "stderr": stderr_log.evidence() if stderr_log is not None else None,
    }
    workspace_cleanup_validated = (
        cleanup_status == "validated"
        and not preserve_workspace
        and result.get("reason") != "variant_exception"
    )
    workspace_cleanup = release_variant_workspace(
        workdir, cleanup_validated=workspace_cleanup_validated
    )
    result["workspace"] = {
        "strategy": "run_directory_local",
        "path": str(workdir),
        "storage_before": (
            prepared.get("workspace_storage_before", workspace_before)
            if prepared is not None
            else workspace_before
        ),
        "storage_after": workspace_storage_evidence(workdir),
        "tempdir": temp_directory_evidence(),
        "directory": workspace_directory_evidence(workdir),
        "cleanup": workspace_cleanup,
    }
    return result


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
    result: dict[str, Any] | None = None
    system_collector: SystemDiagnosticsCollector | None = None
    service_process = unavailable_service_process("service_not_started")

    def finish(payload: dict[str, Any]) -> dict[str, Any]:
        nonlocal result
        result = payload
        return payload

    process: subprocess.Popen[bytes] | None = None
    process_group_id: int | None = None
    output_threads: list[threading.Thread] = []
    stdout_log: BoundedLogWriter | None = None
    stderr_log: BoundedLogWriter | None = None
    prepared: dict[str, Any] | None = None
    stage = "prepare_variant"
    workspace_before = workspace_storage_evidence(workdir)
    try:
        prepared = prepare_variant(
            root, revision, variant, workdir, mock, args, round_number, log_directory
        )
    except VariantPreparationError as failure:
        context = failure.context
        stdout_log = context.get("stdout_log")
        stderr_log = context.get("stderr_log")
        log_cleanup = close_bounded_logs(stdout_log, stderr_log)
        workspace = {
            "strategy": "run_directory_local",
            "path": str(workdir),
            "storage_before": context.get("workspace_storage_before", workspace_before),
            "storage_after": workspace_storage_evidence(workdir),
            "tempdir": temp_directory_evidence(),
            "directory": workspace_directory_evidence(workdir),
            "cleanup": release_variant_workspace(workdir, cleanup_validated=False),
        }
        result = variant_exception_result(
            variant,
            round_number,
            revision,
            args,
            failure.error,
            stage=failure.stage,
            workspace=workspace,
        )
        result["logs"] = {
            "stdout": stdout_log.evidence() if stdout_log is not None else None,
            "stderr": stderr_log.evidence() if stderr_log is not None else None,
        }
        result["cleanup"] = {
            "status": log_cleanup["status"],
            "process": {
                "status": "validated",
                "reason": "service_process_not_started",
                "remaining_pids": [],
                "output_threads_joined": True,
                "errors": [],
            },
            "logs": log_cleanup,
            "errors": log_cleanup.get("errors", []),
        }
        return finish(result)

    workdir = prepared["workdir"]
    repo = prepared["repo"]
    resolved = prepared["resolved"]
    binary_provenance = prepared["binary_provenance"]
    resource_spec = prepared["resource_spec"]
    scenario_shape = prepared["scenario_shape"]
    synthetic_mapping = prepared["synthetic_mapping"]
    core = prepared["core"]
    sidecar = prepared["sidecar"]
    port = prepared["port"]
    metrics_port = prepared["metrics_port"]
    metrics_token = prepared["metrics_token"]
    metrics_token_path = prepared["metrics_token_path"]
    service_command_binding = prepared["service_command_binding"]
    stdout_log = prepared["stdout_log"]
    stderr_log = prepared["stderr_log"]
    if prepared.get("early_reason"):
        reason = prepared["early_reason"]
        early_result = {
            "variant": variant,
            "round": round_number,
            "ref": resolved,
            "status": "unvalidated",
            "reason": reason,
            "duration_seconds": duration_seconds,
            "request": None,
            "unrelated_cohort": None,
            "phases": {},
            "accepted_progress": {"responses": 0, "reported": 0},
            "cohort": logical_cohort(tuple(args.subjects), tuple(args.tokens), args.request_interval_seconds),
            "scenarios": scenario_shape,
            "scenario_contract": scenario_shape,
            "credential_mapping": synthetic_mapping,
            "resource_observation": None,
            "binary_provenance": binary_provenance,
            "service_command_binding": service_command_binding,
            "service_process": service_process,
            "system_diagnostics": unavailable_system_diagnostics(reason),
        }
        return finish(
            finalize_variant_result(
                early_result,
                workdir=workdir,
                workspace_before=workspace_before,
                prepared=prepared,
                process=process,
                process_group_id=process_group_id,
                output_threads=output_threads,
                stdout_log=stdout_log,
                stderr_log=stderr_log,
                system_collector=system_collector,
                binary_provenance=binary_provenance,
                service_command_binding=service_command_binding,
                service_process=service_process,
            )
        )
    stage = "service_process.start"
    try:
        process = subprocess.Popen(
            prepared["command"],
            cwd=repo,
            env=prepared["environment"],
            shell=True,
            start_new_session=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        process_group_id = os.getpgid(process.pid)
        stage = "output_threads.create"
        assert process.stdout is not None
        assert process.stderr is not None
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
        stage = "output_threads.start"
        for thread in output_threads:
            thread.start()
    except (HarnessInterrupted, KeyboardInterrupt):
        interrupted_result = variant_exception_result(
            variant,
            round_number,
            resolved,
            args,
            None,
            stage=stage,
            reason="variant_interrupted",
        )
        finalize_variant_result(
            interrupted_result,
            workdir=workdir,
            workspace_before=workspace_before,
            prepared=prepared,
            process=process,
            process_group_id=process_group_id,
            output_threads=output_threads,
            stdout_log=stdout_log,
            stderr_log=stderr_log,
            system_collector=system_collector,
            binary_provenance=binary_provenance,
            service_command_binding=service_command_binding,
            service_process=service_process,
            preserve_workspace=True,
        )
        raise
    except BaseException as error:
        failed_result = variant_exception_result(
            variant,
            round_number,
            resolved,
            args,
            error,
            stage=stage,
        )
        return finish(
            finalize_variant_result(
                failed_result,
                workdir=workdir,
                workspace_before=workspace_before,
                prepared=prepared,
                process=process,
                process_group_id=process_group_id,
                output_threads=output_threads,
                stdout_log=stdout_log,
                stderr_log=stderr_log,
                system_collector=system_collector,
                binary_provenance=binary_provenance,
                service_command_binding=service_command_binding,
                service_process=service_process,
            )
        )
    try:
        stage = "resource_constraints"
        resource_observation = apply_resource_constraints(process, repo, resource_spec)
        hard_resource_errors = [
            error
            for error in resource_observation["errors"]
            if not error.startswith("sqlite_pool_requested_")
        ]
        if hard_resource_errors:
            return finish({
                "variant": variant,
                "round": round_number,
                "ref": resolved,
                "status": "unvalidated",
                "reason": "resource_constraints_unverified",
                "duration_seconds": duration_seconds,
                "request": None,
                "unrelated_cohort": None,
                "phases": {},
                "accepted_progress": {"responses": 0, "reported": 0},
                "cohort": logical_cohort(
                    tuple(args.subjects), tuple(args.tokens), args.request_interval_seconds
                ),
                "scenarios": scenario_shape,
                "scenario_contract": scenario_shape,
                "credential_mapping": synthetic_mapping,
                "resource_observation": resource_observation,
            })
        stage = "service_readiness"
        service_readiness = wait_for_service(port, args.readiness_timeout_seconds)
        if not service_readiness["listener_ready"]:
            return finish({
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
                "cohort": logical_cohort(
                    tuple(args.subjects), tuple(args.tokens), args.request_interval_seconds
                ),
                "scenarios": scenario_shape,
                "scenario_contract": scenario_shape,
                "credential_mapping": synthetic_mapping,
                "resource_observation": resource_observation,
                "service_readiness": service_readiness,
            })
        stage = "service_process_binding"
        service_process = service_process_binary_binding(process.pid, binary_provenance)
        if service_process.get("status") != "verified":
            return finish({
                "variant": variant,
                "round": round_number,
                "ref": resolved,
                "status": "unvalidated",
                "reason": "service_process_unverified",
                "duration_seconds": duration_seconds,
                "request": None,
                "unrelated_cohort": None,
                "phases": {},
                "accepted_progress": {"responses": 0, "reported": 0},
                "cohort": logical_cohort(
                    tuple(args.subjects), tuple(args.tokens), args.request_interval_seconds
                ),
                "scenarios": scenario_shape,
                "scenario_contract": scenario_shape,
                "credential_mapping": synthetic_mapping,
                "resource_observation": resource_observation,
                "service_readiness": service_readiness,
                "service_process": service_process,
            })
        resource_observation["process"].update(
            {
                "service_pid": service_process["service_pid"],
                "service_exe": service_process["service_exe"],
                "service_binary_sha256": service_process["service_sha256"],
            }
        )
        stage = "system_diagnostics.start"
        system_collector = SystemDiagnosticsCollector(int(service_process["service_pid"]))
        system_collector.start()
        fixture_settings = dict(SYNTHETIC_FIXTURE_SETTINGS)
        fixture_setup: dict[str, Any]
        try:
            stage = "synthetic_fixture_setup"
            configured_settings = configure_synthetic_fixture(port)
        except RuntimeError as error:
            fixture_setup = {
                "status": "failed",
                "reason": str(error),
            }
            return finish({
                "variant": variant,
                "round": round_number,
                "ref": resolved,
                "status": "unvalidated",
                "reason": "synthetic_fixture_setup_failed",
                "duration_seconds": duration_seconds,
                "request": None,
                "unrelated_cohort": None,
                "phases": {},
                "accepted_progress": {"responses": 0, "reported": 0},
                "cohort": logical_cohort(
                    tuple(args.subjects), tuple(args.tokens), args.request_interval_seconds
                ),
                "scenarios": scenario_shape,
                "scenario_contract": scenario_shape,
                "credential_mapping": synthetic_mapping,
                "resource_observation": resource_observation,
                "service_readiness": service_readiness,
                "fixture_setup": fixture_setup,
            })
        fixture_settings.update(configured_settings)
        fixture_setup = {
            "status": "configured",
            "settings": configured_settings,
        }
        credentials = synthetic_credentials(tuple(args.tokens))
        credential_mapping = synthetic_mapping
        if args.bootstrap_bound_business_tokens:
            stage = "bound_business_token_bootstrap"
            credentials, bound_fixture_evidence = bootstrap_bound_business_accounts(
                port,
                core,
                tuple(args.tokens),
                args.request_interval_seconds,
            )
            fixture_setup["bound_account_fixture"] = bound_fixture_evidence
            credential_mapping = credential_mapping_evidence(
                credentials, args.request_interval_seconds
            )
            if bound_fixture_evidence["status"] != "ready":
                return finish({
                    "variant": variant,
                    "round": round_number,
                    "ref": resolved,
                    "status": "unvalidated",
                    "reason": bound_fixture_evidence.get(
                        "reason", "bound_business_fixture_failed"
                    ),
                    "duration_seconds": duration_seconds,
                    "request": None,
                    "unrelated_cohort": None,
                    "phases": {},
                    "accepted_progress": {"responses": 0, "reported": 0},
                    "cohort": logical_cohort(
                        tuple(args.subjects), tuple(args.tokens), args.request_interval_seconds
                    ),
                    "scenarios": scenario_shape,
                    "scenario_contract": scenario_shape,
                    "credential_mapping": credential_mapping,
                    "resource_observation": resource_observation,
                    "service_readiness": service_readiness,
                    "fixture_setup": fixture_setup,
                })
            credential_mapping["runtime_binding_check"] = bound_fixture_evidence.get(
                "runtime_binding_check"
            )
        elif args.bootstrap_business_tokens:
            stage = "business_token_bootstrap"
            credentials, bootstrap_evidence = bootstrap_business_tokens(port, tuple(args.tokens))
            fixture_setup["bootstrap"] = bootstrap_evidence
            if bootstrap_evidence["status"] != "ready":
                return finish({
                    "variant": variant,
                    "round": round_number,
                    "ref": resolved,
                    "status": "unvalidated",
                    "reason": bootstrap_evidence.get(
                        "reason", "business_token_bootstrap_failed"
                    ),
                    "duration_seconds": duration_seconds,
                    "request": None,
                    "unrelated_cohort": None,
                    "phases": {},
                    "accepted_progress": {"responses": 0, "reported": 0},
                    "cohort": logical_cohort(
                        tuple(args.subjects), tuple(args.tokens), args.request_interval_seconds
                    ),
                    "scenarios": scenario_shape,
                    "scenario_contract": scenario_shape,
                    "credential_mapping": synthetic_mapping,
                    "resource_observation": resource_observation,
                    "service_readiness": service_readiness,
                    "fixture_setup": fixture_setup,
                })
            credential_mapping = credential_mapping_evidence(
                credentials, args.request_interval_seconds
            )
            binding_check = verify_credential_bindings(core, credentials)
            credential_mapping["runtime_binding_check"] = binding_check
            if binding_check.get("status") != "passed":
                credential_mapping["status"] = "unverified"
                return finish({
                    "variant": variant,
                    "round": round_number,
                    "ref": resolved,
                    "status": "unvalidated",
                    "reason": "credential_mapping_unverified",
                    "duration_seconds": duration_seconds,
                    "request": None,
                    "unrelated_cohort": None,
                    "phases": {},
                    "accepted_progress": {"responses": 0, "reported": 0},
                    "cohort": logical_cohort(
                        tuple(args.subjects), tuple(args.tokens), args.request_interval_seconds
                    ),
                    "scenarios": scenario_shape,
                    "scenario_contract": scenario_shape,
                    "credential_mapping": credential_mapping,
                    "resource_observation": resource_observation,
                    "service_readiness": service_readiness,
                    "fixture_setup": fixture_setup,
                })
        else:
            fixture_setup["bootstrap"] = {
                "status": "skipped",
                "reason": "business_credentials_not_bootstrapped",
            }
            return finish({
                "variant": variant,
                "round": round_number,
                "ref": resolved,
                "status": "unvalidated",
                "reason": "business_credentials_not_bootstrapped",
                "duration_seconds": duration_seconds,
                "request": None,
                "unrelated_cohort": None,
                "phases": {},
                "accepted_progress": {"responses": 0, "reported": 0},
                "cohort": logical_cohort(
                    tuple(args.subjects), tuple(args.tokens), args.request_interval_seconds
                ),
                "scenarios": scenario_shape,
                "scenario_contract": scenario_shape,
                "credential_mapping": credential_mapping_evidence(
                    credentials, args.request_interval_seconds
                ),
                "resource_observation": resource_observation,
                "service_readiness": service_readiness,
                "fixture_setup": fixture_setup,
            })
        mock_before = mock.evidence()
        stage = "metrics_collector.start"
        metrics_collector = MetricsCollector(
            metrics_port,
            metrics_token,
            args.metrics_scrape_interval_seconds,
        )
        metrics_collector.start()
        stage = "load"
        result, phase_baseline = run_load(
            port,
            args.warmup_seconds,
            duration_seconds,
            tuple(args.subjects),
            credentials,
            args.request_interval_seconds,
            on_measurement_start=lambda: metrics_collector.scrape("measurement_start"),
        )
        metrics_collector.stop()
        result_payload = result
        time.sleep(1.0)
        phase_after = metrics_collector.scrape("after_join_flush")
        result_payload["phases"] = phase_delta(phase_baseline, phase_after)
        result_payload["phase_schema"] = phase_schema_delta(
            phase_baseline, phase_after, result_payload["phases"]
        )
        result_payload["phase_observation"] = {
            "cadence_seconds": args.metrics_scrape_interval_seconds,
            "measurement_start": phase_baseline,
            "after_join_flush": phase_after,
            "observability_delta": observability_delta(phase_baseline, phase_after),
            "collector": metrics_collector.evidence(),
        }
        result = result_payload
        result["fixture_settings"] = fixture_settings
        result.update(
            {
                "variant": variant,
                "round": round_number,
                "ref": resolved,
                "status": "measured",
                "cohort": logical_cohort(
                    tuple(args.subjects), tuple(args.tokens), args.request_interval_seconds
                ),
                "credential_mapping": credential_mapping,
                "resource_observation": resource_observation,
                "service_readiness": service_readiness,
                "fixture_setup": fixture_setup,
                "mock_upstream": {
                    "before": mock_before,
                    "after": mock.evidence(),
                    "response_usage_credits": 1,
                },
            }
        )
        return finish(result)
    except HarnessInterrupted:
        raise
    except Exception as error:
        return finish(
            variant_exception_result(
                variant, round_number, resolved, args, error, stage=stage
            )
        )
    finally:
        if "metrics_collector" in locals():
            metrics_collector.stop()
        if system_collector is not None:
            system_collector.stop()
        cleanup_result = result
        if cleanup_result is None:
            cleanup_result = variant_exception_result(
                variant,
                round_number,
                resolved,
                args,
                None,
                stage=stage,
                reason="variant_interrupted",
            )
        finalize_variant_result(
            cleanup_result,
            workdir=workdir,
            workspace_before=workspace_before,
            prepared=prepared,
            process=process,
            process_group_id=process_group_id,
            output_threads=output_threads,
            stdout_log=stdout_log,
            stderr_log=stderr_log,
            system_collector=system_collector,
            binary_provenance=binary_provenance,
            service_command_binding=service_command_binding,
            service_process=service_process,
            preserve_workspace=result is None,
        )


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
    parser.add_argument("--min-control-samples", type=int, default=DEFAULT_MIN_SAMPLES)
    parser.add_argument("--request-interval-seconds", type=float, default=DEFAULT_REQUEST_INTERVAL_SECONDS)
    parser.add_argument(
        "--metrics-scrape-interval-seconds",
        type=float,
        default=DEFAULT_METRICS_SCRAPE_INTERVAL_SECONDS,
        help="bounded metrics collector cadence; zero keeps boundary-only scrapes",
    )
    parser.add_argument("--readiness-timeout-seconds", type=float, default=60)
    parser.add_argument(
        "--service-command",
        help=(
            "shell template; supports {repo}, {db}, {sidecar}, {port}, {upstream}, "
            "{variant}, {binary}, {manifest}, {revision}"
        ),
    )
    parser.add_argument(
        "--binary-manifest",
        help="JSON build manifest template; supports {repo}, {variant}, {revision}",
    )
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
    parser.add_argument(
        "--bootstrap-bound-business-tokens",
        action=argparse.BooleanOptionalAction,
        default=False,
        help="seed 14 deterministic access tokens bound to synthetic accounts with quota headroom",
    )
    return parser.parse_args()


def validate_metrics_scrape_interval(interval_seconds: float, metrics_enabled: bool) -> None:
    if (
        interval_seconds != interval_seconds
        or interval_seconds < 0
        or interval_seconds > MAX_METRICS_SCRAPE_INTERVAL_SECONDS
    ):
        raise SystemExit(
            "metrics scrape interval must be between 1 and 60 seconds when enabled"
        )
    if interval_seconds == 0:
        if metrics_enabled:
            raise SystemExit(
                "metrics-enabled runs require a positive bounded metrics scrape cadence"
            )
        return
    if interval_seconds < MIN_METRICS_SCRAPE_INTERVAL_SECONDS:
        raise SystemExit(
            "metrics scrape interval must be 0 when disabled or between 1 and 60 seconds"
        )


def validate_args(args: argparse.Namespace) -> None:
    if args.rounds < 1:
        raise SystemExit("--rounds must be positive")
    if args.warmup_seconds < 0 or args.duration_seconds < 1:
        raise SystemExit("warmup must be non-negative and duration must be positive")
    if args.duration_seconds > args.extension_seconds:
        raise SystemExit("duration cannot exceed extension duration")
    if args.min_samples < 1 or args.min_control_samples < 1:
        raise SystemExit("sample minimums must be positive")
    if args.request_interval_seconds <= 0:
        raise SystemExit("request interval must be positive")
    validate_metrics_scrape_interval(
        args.metrics_scrape_interval_seconds,
        args.metrics_enabled,
    )
    if args.mock_delay_ms != DEFAULT_MOCK_DELAY_MS:
        raise SystemExit("the local boundary contract fixes mock delay at 200ms")
    if args.bootstrap_bound_business_tokens and args.bootstrap_business_tokens:
        raise SystemExit("bound and unbound business token bootstrap are mutually exclusive")
    if not args.subjects or not args.tokens:
        raise SystemExit("subject and token distributions must be non-empty")
    if len(args.subjects) < REQUIRED_IDENTITY_COUNT or len(args.tokens) < REQUIRED_IDENTITY_COUNT:
        raise SystemExit(
            f"at least {REQUIRED_IDENTITY_COUNT} subjects and token labels are required for isolated scenarios"
        )
    if len(set(args.subjects[:REQUIRED_IDENTITY_COUNT])) != REQUIRED_IDENTITY_COUNT:
        raise SystemExit("the first 14 subjects must be distinct across workload scenarios")
    if len(set(args.tokens[:REQUIRED_IDENTITY_COUNT])) != REQUIRED_IDENTITY_COUNT:
        raise SystemExit("the first 14 token labels must be distinct across workload scenarios")
    if any(not re.fullmatch(r"subject-[a-z0-9-]+", value) for value in args.subjects):
        raise SystemExit("subjects must use synthetic subject-* values")
    if any(not re.fullmatch(r"token-[a-z0-9-]+", value) for value in args.tokens):
        raise SystemExit("tokens must use synthetic token-* values")
    if not args.comparison_entry.is_file():
        raise SystemExit(f"comparison entry does not exist: {args.comparison_entry}")
    try:
        args.resource_spec = parse_resource_spec(args.resources)
    except ValueError as error:
        raise SystemExit(str(error)) from error


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
        or reason.endswith("/control.samples_below_minimum")
        or reason.endswith("/control/p95_regression_over_5_percent")
        or reason.endswith("/control/baseline_p95_variation_over_5_percent")
        or reason.endswith("/control/candidate_p95_variation_over_5_percent")
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
        checkpoint.write_variant_started(
            phase, round_number, "baseline", args.baseline_ref, duration_seconds
        )
        try:
            baseline = run_variant(
                root,
                args.baseline_ref,
                "baseline",
                variant_workspace_path(workdir, phase, round_number, "baseline"),
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
                "baseline",
                round_number,
                args.baseline_ref,
                args,
                error,
                stage="run_variant.wrapper",
            )
        checkpoint.write_variant_result(phase, round_number, "baseline", baseline)

        checkpoint.write_variant_started(
            phase, round_number, "candidate", args.candidate_ref, duration_seconds
        )
        if baseline.get("cleanup", {}).get("status") != "validated":
            candidate = variant_exception_result(
                "candidate",
                round_number,
                args.candidate_ref,
                args,
                None,
                stage="baseline.cleanup",
                reason="prior_cleanup_unvalidated",
            )
            candidate["blocked_by"] = "baseline"
            candidate["cleanup"] = {
                "status": "not_run",
                "reason": "blocked_by_baseline_cleanup",
            }
        else:
            try:
                candidate = run_variant(
                    root,
                    args.candidate_ref,
                    "candidate",
                    variant_workspace_path(workdir, phase, round_number, "candidate"),
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
                    "candidate",
                    round_number,
                    args.candidate_ref,
                    args,
                    error,
                    stage="run_variant.wrapper",
                )
        checkpoint.write_variant_result(phase, round_number, "candidate", candidate)

        round_record = {"round": round_number, "baseline": baseline, "candidate": candidate}
        paired_rounds.append(round_record)
        round_comparison = compare_rounds(
            [round_record], args.min_samples, args.min_control_samples
        )["rounds"][0]
        checkpoint.write_pair(phase, round_number, baseline, candidate, round_comparison)
        if candidate.get("cleanup", {}).get("status") != "validated":
            break
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
        "success_request": result.get("success_request"),
        "unrelated_cohort": result.get("unrelated_cohort"),
        "statuses": result.get("statuses", {}),
        "outcome_counts": result.get("outcome_counts", {}),
        "business_outcomes": result.get("business_outcomes", {}),
        "business_statuses": result.get("business_statuses", {}),
        "response_evidence": result.get("response_evidence", []),
        "errors": result.get("errors", {}),
        "phases": result.get("phases", {}),
        "phase_schema": result.get("phase_schema"),
        "warmup": result.get("warmup"),
        "mock_upstream": result.get("mock_upstream"),
        "cohort": result.get("cohort"),
        "scenarios": result.get("scenarios"),
        "scenario_contract": result.get("scenario_contract"),
        "scenario_digest": result.get("scenario_digest"),
        "credential_mapping": result.get("credential_mapping"),
        "resource_observation": result.get("resource_observation"),
        "accepted_progress": result.get("accepted_progress"),
        "fixture_settings": result.get("fixture_settings"),
        "fixture_setup": result.get("fixture_setup"),
    }


def scenario_shape_key(result: dict[str, Any]) -> str | None:
    scenarios = result.get("scenario_contract")
    if not isinstance(scenarios, list):
        return None
    shape = [
        {
            "name": item.get("name"),
            "driver": item.get("driver"),
            "concurrency": item.get("concurrency"),
            "credential_mode": item.get("credential_mode"),
            "credential_slots": item.get("credential_slots"),
            "credential_indices": item.get("credential_indices"),
            "subject_indices": item.get("subject_indices"),
            "expected_outcome": item.get("expected_outcome"),
            "expected_status_class": item.get("expected_status_class"),
            "scheduling": item.get("scheduling"),
        }
        for item in scenarios
        if isinstance(item, dict)
    ]
    return json.dumps(shape, sort_keys=True, separators=(",", ":"))


def scenario_identity_reasons(
    prefix: str,
    scenarios: list[dict[str, Any]],
    records_by_name: dict[str, dict[str, Any]],
) -> list[str]:
    reasons: list[str] = []
    credential_id_owners: dict[str, set[str]] = {}
    credential_owners: dict[str, set[str]] = {}
    subject_owners: dict[str, set[str]] = {}
    for scenario in scenarios:
        name = scenario.get("name")
        record = records_by_name.get(name)
        credential_indices = scenario.get("credential_indices")
        subject_indices = scenario.get("subject_indices")
        if (
            not isinstance(name, str)
            or not isinstance(record, dict)
            or not isinstance(credential_indices, list)
            or not isinstance(subject_indices, list)
        ):
            reasons.append(f"{prefix}/scenarios/{name}/identity_contract_missing")
            continue
        for field, expected_indices in (
            ("credential_ids", credential_indices),
            ("billing_subjects", credential_indices),
            ("subjects", subject_indices),
        ):
            values = record.get(field)
            if not isinstance(values, list) or not all(
                isinstance(value, str) and value for value in values
            ):
                reasons.append(f"{prefix}/scenarios/{name}/identity/{field}_missing")
                continue
            if len(values) != len(expected_indices):
                reasons.append(f"{prefix}/scenarios/{name}/identity/{field}_count_mismatch")
            if len(set(values)) != len(set(expected_indices)):
                reasons.append(f"{prefix}/scenarios/{name}/identity/{field}_reuse_mismatch")
            if field == "subjects":
                subject_owners[name] = set(values)
            elif field == "credential_ids":
                credential_id_owners[name] = set(values)
            elif field == "billing_subjects":
                credential_owners[name] = set(values)

    for owners, reason_suffix in (
        (credential_id_owners, "credential_reused_with"),
        (credential_owners, "billing_subject_reused_with"),
    ):
        scenario_names = list(owners)
        for index, left_name in enumerate(scenario_names):
            for right_name in scenario_names[index + 1 :]:
                if owners[left_name].isdisjoint(owners[right_name]):
                    continue
                reasons.append(
                    f"{prefix}/scenarios/{left_name}/identity/{reason_suffix}_{right_name}"
                )
    scenario_names = list(subject_owners)
    for index, left_name in enumerate(scenario_names):
        for right_name in scenario_names[index + 1 :]:
            if subject_owners[left_name].isdisjoint(subject_owners[right_name]):
                continue
            reasons.append(f"{prefix}/scenarios/{left_name}/identity/subject_reused_with_{right_name}")
    return reasons


def measurement_reasons(
    round_number: int,
    role: str,
    result: dict[str, Any],
    min_samples: int,
    min_control_samples: int | None = None,
) -> list[str]:
    if min_control_samples is None:
        min_control_samples = min_samples
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
        elif control.get("samples", 0) < min_control_samples:
            reasons.append(f"{prefix}/control.samples_below_minimum")
        elif control.get("p95_ms") is None:
            reasons.append(f"{prefix}/control.p95_missing")
    success_request = result.get("success_request")
    if not isinstance(success_request, dict):
        reasons.append(f"{prefix}/business/success.missing")
    else:
        if success_request.get("samples", 0) <= 0:
            reasons.append(f"{prefix}/business/success.samples_missing")
        elif success_request.get("samples", 0) < min_samples:
            reasons.append(f"{prefix}/business/success.samples_below_minimum")
        elif success_request.get("p95_ms") is None:
            reasons.append(f"{prefix}/business/success.p95_missing")
    business_outcomes = result.get("business_outcomes")
    if not isinstance(business_outcomes, dict):
        reasons.append(f"{prefix}/business/outcomes_missing")
    elif not isinstance(business_outcomes.get("success"), int):
        reasons.append(f"{prefix}/business/success_count_missing")
    elif business_outcomes.get("success", 0) <= 0:
        reasons.append(f"{prefix}/business/429_or_error_only")
    scenarios = result.get("scenario_contract")
    scenario_names = [
        item.get("name") for item in scenarios if isinstance(item, dict)
    ] if isinstance(scenarios, list) else []
    if scenario_names != list(REQUIRED_SCENARIO_NAMES):
        reasons.append(f"{prefix}/scenarios/contract_mismatch")
    scenario_records = result.get("scenarios")
    if not isinstance(scenario_records, list):
        reasons.append(f"{prefix}/scenarios/records_missing")
    else:
        records_by_name = {
            record.get("name"): record
            for record in scenario_records
            if isinstance(record, dict) and record.get("name")
        }
        for scenario in scenarios if isinstance(scenarios, list) else []:
            if not isinstance(scenario, dict):
                continue
            name = scenario.get("name")
            record = records_by_name.get(name)
            if record is None:
                reasons.append(f"{prefix}/scenarios/{name}/record_missing")
                continue
            expected = record.get("expected")
            if not isinstance(expected, dict):
                reasons.append(f"{prefix}/scenarios/{name}/expected_missing")
            elif (
                expected.get("outcome") != scenario.get("expected_outcome")
                or expected.get("status_class") != scenario.get("expected_status_class")
            ):
                reasons.append(f"{prefix}/scenarios/{name}/expected_mismatch")
            actual = record.get("actual")
            if not isinstance(actual, dict):
                reasons.append(f"{prefix}/scenarios/{name}/actual_missing")
                continue
            outcomes = actual.get("outcomes")
            if not isinstance(outcomes, dict) or any(
                not isinstance(outcomes.get(outcome), int)
                for outcome in ("success", "reject", "error")
            ):
                reasons.append(f"{prefix}/scenarios/{name}/outcomes_missing")
            elif outcomes.get("success", 0) <= 0:
                reasons.append(f"{prefix}/scenarios/{name}/success.samples_missing")
            scenario_success = actual.get("success_request")
            if not isinstance(scenario_success, dict):
                reasons.append(f"{prefix}/scenarios/{name}/success.missing")
            elif scenario_success.get("samples", 0) < min_samples:
                reasons.append(f"{prefix}/scenarios/{name}/success.samples_below_minimum")
            elif scenario_success.get("p95_ms") is None:
                reasons.append(f"{prefix}/scenarios/{name}/success.p95_missing")
        if isinstance(scenarios, list):
            reasons.extend(scenario_identity_reasons(prefix, scenarios, records_by_name))
    phase_schema = result.get("phase_schema")
    if not isinstance(phase_schema, dict):
        reasons.append(f"{prefix}/phase_schema/missing")
    elif phase_schema.get("status") != "supported":
        reasons.append(
            f"{prefix}/phase_schema/{phase_schema.get('reason') or 'unsupported'}"
        )
    elif phase_schema.get("coverage_count", 0) <= 0:
        reasons.append(f"{prefix}/phase_schema/coverage_missing")
    elif isinstance(success_request, dict) and phase_schema.get("coverage_count", 0) < success_request.get("samples", 0):
        reasons.append(f"{prefix}/phase_schema/coverage_below_success_samples")
    mapping = result.get("credential_mapping")
    if not isinstance(mapping, dict):
        reasons.append(f"{prefix}/credentials/mapping_missing")
    elif mapping.get("status") != "verified":
        reasons.append(f"{prefix}/credentials/mapping_unverified")
    resource_observation = result.get("resource_observation")
    if not isinstance(resource_observation, dict):
        reasons.append(f"{prefix}/resources/observation_missing")
    elif resource_observation.get("status") != "validated":
        reasons.append(f"{prefix}/resources/constraints_unverified")
    binary_provenance = result.get("binary_provenance")
    if not isinstance(binary_provenance, dict):
        reasons.append(f"{prefix}/binary_provenance/missing")
    elif binary_provenance.get("status") != "verified":
        reasons.append(
            f"{prefix}/binary_provenance/{binary_provenance.get('reason') or 'unverified'}"
        )
    command_binding = result.get("service_command_binding")
    if not isinstance(command_binding, dict):
        reasons.append(f"{prefix}/service_command/binary_binding_missing")
    elif command_binding.get("status") not in {
        "explicit_placeholder",
        "explicit_environment",
    }:
        reasons.append(
            f"{prefix}/service_command/{command_binding.get('reason') or 'binary_binding_unverified'}"
        )
    service_process = result.get("service_process")
    if not isinstance(service_process, dict):
        reasons.append(f"{prefix}/service_process/missing")
    elif service_process.get("status") != "verified":
        reasons.append(
            f"{prefix}/service_process/{service_process.get('reason') or 'unverified'}"
        )
    else:
        service_pid = service_process.get("service_pid")
        if not isinstance(service_pid, int) or service_pid <= 0:
            reasons.append(f"{prefix}/service_process/service_pid_invalid")
        if not isinstance(service_process.get("service_exe"), str) or not service_process.get(
            "service_exe"
        ):
            reasons.append(f"{prefix}/service_process/service_exe_missing")
        expected_binary_sha256 = (
            binary_provenance.get("binary_sha256")
            if isinstance(binary_provenance, dict)
            else None
        )
        if service_process.get("service_sha256") != expected_binary_sha256:
            reasons.append(f"{prefix}/service_process/binary_hash_mismatch")
    system_diagnostics = result.get("system_diagnostics")
    if not isinstance(system_diagnostics, dict):
        reasons.append(f"{prefix}/system_diagnostics/missing")
    elif system_diagnostics.get("status") != "validated":
        reasons.append(
            f"{prefix}/system_diagnostics/{system_diagnostics.get('reason') or 'incomplete'}"
        )
    elif isinstance(service_process, dict) and system_diagnostics.get("pid") != service_process.get(
        "service_pid"
    ):
        reasons.append(f"{prefix}/system_diagnostics/service_pid_mismatch")
    if cohort_key(result) is None:
        reasons.append(f"{prefix}/cohort.missing")
    return reasons


def compare_round(
    round_record: dict[str, Any],
    min_samples: int,
    min_control_samples: int | None = None,
    expected_cohort: str | None = None,
    expected_scenario_shape: str | None = None,
) -> dict[str, Any]:
    if min_control_samples is None:
        min_control_samples = min_samples
    round_number = int(round_record["round"])
    baseline = round_record["baseline"]
    candidate = round_record["candidate"]
    reasons = measurement_reasons(
        round_number, "baseline", baseline, min_samples, min_control_samples
    )
    reasons.extend(
        measurement_reasons(round_number, "candidate", candidate, min_samples, min_control_samples)
    )
    baseline_cohort = cohort_key(baseline)
    candidate_cohort = cohort_key(candidate)
    if baseline_cohort is not None and candidate_cohort is not None:
        if baseline_cohort != candidate_cohort:
            reasons.append(f"round-{round_number}/cohort/mismatch")
        if expected_cohort is not None and baseline_cohort != expected_cohort:
            reasons.append(f"round-{round_number}/cohort/changed_across_rounds")
    baseline_scenario_shape = scenario_shape_key(baseline)
    candidate_scenario_shape = scenario_shape_key(candidate)
    if (
        baseline_scenario_shape is not None
        and candidate_scenario_shape is not None
        and baseline_scenario_shape != candidate_scenario_shape
    ):
        reasons.append(f"round-{round_number}/scenarios/mismatch")
    if expected_scenario_shape is not None and baseline_scenario_shape != expected_scenario_shape:
        reasons.append(f"round-{round_number}/scenarios/changed_across_rounds")

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

    baseline_request = baseline["success_request"]
    candidate_request = candidate["success_request"]
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


def variation_ratio(values: list[float]) -> float | None:
    if len(values) < 2:
        return 0.0 if values else None
    center = statistics.median(values)
    if center <= 0:
        return None
    return max(abs(value / center - 1) for value in values)


def compare_rounds(
    rounds: list[dict[str, Any]],
    min_samples: int,
    min_control_samples: int | None = None,
) -> dict[str, Any]:
    if min_control_samples is None:
        min_control_samples = min_samples
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
    expected_scenario_shape: str | None = None
    for round_record in rounds:
        comparison = compare_round(
            round_record,
            min_samples,
            min_control_samples,
            expected_cohort,
            expected_scenario_shape,
        )
        comparisons.append(comparison)
        for reason in comparison["reasons"]:
            if reason not in reasons:
                reasons.append(reason)
        if expected_cohort is None:
            expected_cohort = cohort_key(round_record["baseline"])
        if expected_scenario_shape is None:
            expected_scenario_shape = scenario_shape_key(round_record["baseline"])

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
    control_complete = (
        len(control_baseline) == len(comparisons)
        and len(control_candidate) == len(comparisons)
        and bool(comparisons)
    )
    baseline_control_variation = variation_ratio(control_baseline) if control_complete else None
    candidate_control_variation = variation_ratio(control_candidate) if control_complete else None
    control_stability = {
        "min_samples": min_control_samples,
        "rounds": len(control_baseline),
        "baseline_p95_variation_ratio": baseline_control_variation,
        "candidate_p95_variation_ratio": candidate_control_variation,
        "stable": control_complete,
    }
    if control_complete and (baseline_control_variation is None or baseline_control_variation > 0.05):
        control_stability["stable"] = False
        reason = "rounds/control/baseline_p95_variation_over_5_percent"
        if reason not in reasons:
            reasons.append(reason)
    if control_complete and (
        candidate_control_variation is None or candidate_control_variation > 0.05
    ):
        control_stability["stable"] = False
        reason = "rounds/control/candidate_p95_variation_over_5_percent"
        if reason not in reasons:
            reasons.append(reason)
    summary = {
        "p95_is_descriptive_only": True,
        "p95_basis": "business_success_only",
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
        "control_stability": control_stability,
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
    error: BaseException | None,
    *,
    stage: str | None = None,
    reason: str = "variant_exception",
    cleanup: dict[str, Any] | None = None,
    workspace: dict[str, Any] | None = None,
) -> dict[str, Any]:
    result: dict[str, Any] = {
        "variant": variant,
        "round": round_number,
        "ref": ref,
        "status": "unvalidated",
        "reason": reason,
        "duration_seconds": 0,
        "request": None,
        "unrelated_cohort": None,
        "statuses": {},
        "errors": {},
        "phases": {},
        "accepted_progress": {"responses": 0, "reported": 0},
        "cohort": logical_cohort(
            tuple(args.subjects), tuple(args.tokens), args.request_interval_seconds
        ),
        "scenarios": scenario_contract(args.request_interval_seconds),
        "scenario_contract": scenario_contract(args.request_interval_seconds),
        "credential_mapping": credential_mapping_metadata(args),
        "resource_observation": None,
        "binary_provenance": {
            "status": "missing",
            "reason": "variant_exception",
        },
        "service_command_binding": {
            "status": "unavailable",
            "reason": "variant_exception",
        },
        "service_process": unavailable_service_process("variant_exception"),
        "system_diagnostics": unavailable_system_diagnostics("variant_exception"),
    }
    if error is not None:
        result["error_type"] = type(error).__name__
        result["error"] = exception_evidence(error, stage)
    if stage is not None:
        result["failure_stage"] = stage
    if cleanup is not None:
        result["cleanup"] = cleanup
    if workspace is not None:
        result["workspace"] = workspace
    return result


def main() -> int:
    args = parse_args()
    validate_args(args)
    root = Path(__file__).resolve().parents[1]
    output = args.output.resolve()
    run_directory = create_run_directory(output, args.run_directory)
    workspace_root = run_directory / ".work"
    workspace_root.mkdir(parents=True, exist_ok=True)
    contract = {
        "baseline_ref": args.baseline_ref,
        "candidate_ref": args.candidate_ref,
        "warmup_seconds": args.warmup_seconds,
        "duration_seconds": args.duration_seconds,
        "rounds": args.rounds,
        "mock_delay_ms": args.mock_delay_ms,
        "resources": args.resources,
        "resource_spec": args.resource_spec,
        "subjects": args.subjects,
        "tokens": args.tokens,
        "scenarios": scenario_contract(args.request_interval_seconds),
        "cohort_path": "variants[*].scenarios[*].request",
        "control_path": "variants[*].unrelated_cohort",
        "credential_mapping": credential_mapping_metadata(args),
        "fixture_settings": dict(SYNTHETIC_FIXTURE_SETTINGS),
        "scheduling": {
            "request_interval_seconds": args.request_interval_seconds,
            "control_interval_seconds": DEFAULT_CONTROL_INTERVAL_SECONDS,
            "metrics_scrape_interval_seconds": args.metrics_scrape_interval_seconds,
            "metrics_scrape_cadence_mode": (
                "bounded_post_scrape"
                if args.metrics_scrape_interval_seconds > 0
                else "boundary_only"
            ),
            "system_diagnostics_interval_seconds": DEFAULT_SYSTEM_DIAGNOSTICS_INTERVAL_SECONDS,
            "measurement_window": "shared_per_variant",
        },
        "metrics_coverage_contract": {
            "minimum_scrape_interval_seconds": MIN_METRICS_SCRAPE_INTERVAL_SECONDS,
            "maximum_scrape_interval_seconds": MAX_METRICS_SCRAPE_INTERVAL_SECONDS,
            "requirement": "phase_schema.coverage_count >= success_request.samples",
        },
        "min_samples": args.min_samples,
        "min_control_samples": args.min_control_samples,
        "bootstrap_business_tokens": args.bootstrap_business_tokens,
        "metrics_enabled": args.metrics_enabled,
        "comparison_entry": str(args.comparison_entry.resolve()),
        "binary_manifest": args.binary_manifest,
        "upstream_kind": "loopback-synthetic-mock",
        "phase_metric_selector": PHASE_METRIC_SELECTOR,
        "phase_metric_schema_selectors": {
            PHASE_SCHEMA_LEGACY: LEGACY_PHASE_METRIC_SELECTOR,
            PHASE_SCHEMA_OPERATION: PHASE_METRIC_SELECTOR,
        },
        "run_directory": str(run_directory),
        "workspace": {
            "strategy": "run_directory_local",
            "root": str(workspace_root),
            "storage": workspace_storage_evidence(workspace_root),
            "tempdir": temp_directory_evidence(),
        },
        "checkpoint_schema": 1,
    }
    checkpoint = CheckpointStore(run_directory, contract)
    evidence: dict[str, Any] = {
        "contract": contract,
        "rounds": [],
        "comparison": None,
        "status": "running",
        "workspace": {
            "strategy": "run_directory_local",
            "root": str(workspace_root),
            "storage": workspace_storage_evidence(workspace_root),
            "tempdir": temp_directory_evidence(),
            "directory": workspace_directory_evidence(workspace_root),
        },
        "checkpoint": {
            "run_directory": str(run_directory),
            "contract": str(run_directory / "contract.json"),
            "variants": str(checkpoint.variant_root),
            "pairs": str(checkpoint.pair_root),
            "phases": str(checkpoint.phase_root),
            "logs": str(checkpoint.log_root),
            "progress": str(run_directory / "progress.json"),
        },
    }
    previous_handlers = install_interrupt_handlers()
    exit_code = 0
    try:
        with MockUpstream(args.mock_delay_ms) as mock:
            evidence["rounds"] = run_paired_rounds(
                root,
                run_directory,
                mock,
                args,
                args.duration_seconds,
                "initial",
                checkpoint,
            )
            comparison = compare_rounds(
                evidence["rounds"], args.min_samples, args.min_control_samples
            )
            evidence["comparison"] = comparison
            evidence["status"] = comparison["status"]
            checkpoint.write_phase_comparison("initial", evidence["rounds"], comparison)
            if comparison["status"] == "unvalidated" and args.duration_seconds < args.extension_seconds:
                cleanup_blocked = any(
                    variant.get("cleanup", {}).get("status") != "validated"
                    for round_record in evidence["rounds"]
                    for variant in (round_record["baseline"], round_record["candidate"])
                )
                if cleanup_blocked:
                    evidence["extension"] = {
                        "status": "not_allowed",
                        "duration_seconds": args.extension_seconds,
                        "reason": "cleanup_unvalidated",
                        "initial_reasons": comparison["reasons"],
                    }
                elif args.service_command and extension_allowed(comparison):
                    extension_rounds = run_paired_rounds(
                        root,
                        run_directory,
                        mock,
                        args,
                        args.extension_seconds,
                        "extension",
                        checkpoint,
                    )
                    extension_comparison = compare_rounds(
                        extension_rounds, args.min_samples, args.min_control_samples
                    )
                    checkpoint.write_phase_comparison(
                        "extension", extension_rounds, extension_comparison
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
        evidence["workspace"].update(
            {
                "storage": workspace_storage_evidence(workspace_root),
                "directory": workspace_directory_evidence(workspace_root),
            }
        )
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
        evidence["workspace"].update(
            {
                "storage": workspace_storage_evidence(workspace_root),
                "directory": workspace_directory_evidence(workspace_root),
            }
        )
        restore_interrupt_handlers(previous_handlers)
    atomic_write_json(output, evidence)
    print(json.dumps(evidence, indent=2, sort_keys=True))
    if exit_code:
        return exit_code
    return 1 if evidence["status"] == "failed" else 0


if __name__ == "__main__":
    raise SystemExit(main())
