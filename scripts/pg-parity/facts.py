#!/usr/bin/env python3
"""Export comparable fact tables from a SQLite-backed or PostgreSQL-backed store.

The fixture intentionally queries the *stored* schema, not the HTTP read models:
the point is to prove both backends persist the same facts for one deterministic
traffic run.

Backend differences that the schema forces:

* SQLite keeps observability tables in an attached sidecar file. The sidecar path
  mirrors `SqliteDatabaseLayout` in `src/store/mod.rs`: `<stem>-observability.db`.
* PostgreSQL keeps them under the `observability` schema.
* `api_key_usage_buckets` has no credit column on either backend. Local estimated
  credits are stored on `dashboard_request_rollup_buckets`, so the per-day credit
  sum is read from that table.
* This repository has no `accounts.balance` relation. The account balance fact is
  the key-pool balance (`api_keys.quota_remaining`), which is what the service
  actually reconciles today.
* Alert events carry their grouping identity inside `payload_json`. The
  `group_key` fact is `payload_json.alert_type` and `status` is
  `payload_json.result_status` (falling back to `payload_json.job_status`).

`day_key` is the UTC calendar date of the stored `bucket_start`. The bucket start
itself uses the server-local day boundary; the label is UTC so the comparison is
independent of the timezone of the machine or database session doing the export.
"""

from __future__ import annotations

import json
import os
import sqlite3
import subprocess
from pathlib import Path
from typing import Any

SQLITE_CORE_SCHEMA = ""
SQLITE_OBSERVABILITY_SCHEMA = "observability."


class FactsError(RuntimeError):
    """Raised when a backend's facts cannot be exported."""


def sidecar_path(core_db_path: Path) -> Path:
    """Mirror `sqlite_sidecar_path(core, "observability.db")` from src/store/mod.rs."""
    return core_db_path.with_name(f"{core_db_path.stem}-observability.db")


# ---------------------------------------------------------------------------
# SQLite
# ---------------------------------------------------------------------------


def _sqlite_rows(conn: sqlite3.Connection, sql: str) -> list[tuple[Any, ...]]:
    try:
        return list(conn.execute(sql))
    except sqlite3.Error as error:
        raise FactsError(f"sqlite query failed: {error}\nSQL: {sql}") from error


def _sqlite_scalar(conn: sqlite3.Connection, sql: str) -> Any:
    rows = _sqlite_rows(conn, sql)
    return rows[0][0] if rows else None


def _sqlite_raw(core_db_path: Path) -> dict[str, Any]:
    core_db_path = core_db_path.resolve()
    if not core_db_path.is_file():
        raise FactsError(f"sqlite core database is missing: {core_db_path}")
    observability_db_path = sidecar_path(core_db_path)
    if not observability_db_path.is_file():
        raise FactsError(
            f"sqlite observability sidecar is missing: {observability_db_path}"
        )

    conn = sqlite3.connect(str(core_db_path))
    try:
        conn.execute("ATTACH DATABASE ? AS observability", (str(observability_db_path),))
        raw: dict[str, Any] = {
            "auth_total": int(
                _sqlite_scalar(conn, "SELECT COUNT(*) FROM auth_token_logs")
            ),
            "auth_by_result": _sqlite_rows(
                conn,
                """
                SELECT COALESCE(NULLIF(TRIM(result_status), ''), 'unknown') AS key,
                       COUNT(*) AS n
                  FROM auth_token_logs
                 GROUP BY key
                 ORDER BY key
                """,
            ),
            "auth_by_kind": _sqlite_rows(
                conn,
                """
                SELECT COALESCE(NULLIF(TRIM(request_kind_key), ''), 'unknown') AS key,
                       COUNT(*) AS n
                  FROM auth_token_logs
                 GROUP BY key
                 ORDER BY key
                """,
            ),
            "logs_total": int(
                _sqlite_scalar(
                    conn, "SELECT COUNT(*) FROM observability.request_logs"
                )
            ),
            "logs_by_visibility": _sqlite_rows(
                conn,
                """
                SELECT COALESCE(NULLIF(TRIM(visibility), ''), 'unknown') AS key,
                       COUNT(*) AS n
                  FROM observability.request_logs
                 GROUP BY key
                 ORDER BY key
                """,
            ),
            "bucket_days": _sqlite_rows(
                conn,
                """
                SELECT date(bucket_start, 'unixepoch') AS day_key,
                       SUM(total_requests) AS total_requests
                  FROM observability.api_key_usage_buckets
                 GROUP BY day_key
                 ORDER BY day_key
                """,
            ),
            "rollup_days": _sqlite_rows(
                conn,
                """
                SELECT date(bucket_start, 'unixepoch') AS day_key,
                       SUM(local_estimated_credits) AS local_estimated_credits
                  FROM observability.dashboard_request_rollup_buckets
                 WHERE bucket_secs = 86400
                 GROUP BY day_key
                 ORDER BY day_key
                """,
            ),
            "balance_total": int(
                _sqlite_scalar(
                    conn,
                    """
                    SELECT COALESCE(SUM(quota_remaining), 0)
                      FROM api_keys
                     WHERE deleted_at IS NULL
                    """,
                )
            ),
            "alert_total": int(
                _sqlite_scalar(
                    conn,
                    "SELECT COUNT(*) FROM observability.dashboard_alert_projection_events",
                )
            ),
            "alert_rows": _sqlite_rows(
                conn,
                """
                SELECT COALESCE(json_extract(payload_json, '$.alert_type'), 'unknown') AS group_key,
                       COALESCE(
                           json_extract(payload_json, '$.result_status'),
                           json_extract(payload_json, '$.job_status'),
                           'unknown'
                       ) AS status,
                       COUNT(*) AS n
                  FROM observability.dashboard_alert_projection_events
                 GROUP BY group_key, status
                 ORDER BY group_key, status
                """,
            ),
            "invariant": _sqlite_invariant(conn),
        }
        return raw
    finally:
        conn.close()


def _sqlite_invariant(conn: sqlite3.Connection) -> dict[str, Any]:
    bucket_total = int(
        _sqlite_scalar(
            conn,
            "SELECT COALESCE(SUM(total_requests), 0) FROM observability.api_key_usage_buckets",
        )
    )
    window = _sqlite_rows(
        conn,
        """
        SELECT MIN(bucket_start), MAX(bucket_start), MAX(bucket_secs)
          FROM observability.api_key_usage_buckets
        """,
    )[0]
    window_min, window_max, window_secs = window
    if window_min is None or window_max is None or window_secs is None:
        visible_keyed = int(
            _sqlite_scalar(
                conn,
                """
                SELECT COUNT(*)
                  FROM observability.request_logs
                 WHERE visibility = 'visible' AND api_key_id IS NOT NULL
                """,
            )
        )
        return {
            "bucket_total_requests": bucket_total,
            "request_logs_visible_with_key": visible_keyed,
            "window_start": None,
            "window_end": None,
            "window_secs": None,
        }
    window_end = int(window_max) + int(window_secs)
    visible_keyed = int(
        _sqlite_scalar(
            conn,
            f"""
            SELECT COUNT(*)
              FROM observability.request_logs
             WHERE visibility = 'visible'
               AND api_key_id IS NOT NULL
               AND created_at >= {int(window_min)}
               AND created_at < {window_end}
            """,
        )
    )
    return {
        "bucket_total_requests": bucket_total,
        "request_logs_visible_with_key": visible_keyed,
        "window_start": int(window_min),
        "window_end": window_end,
        "window_secs": int(window_secs),
    }


# ---------------------------------------------------------------------------
# PostgreSQL
# ---------------------------------------------------------------------------


def _pg_json(pg_url: str, sql: str) -> Any:
    try:
        result = subprocess.run(
            [
                "psql",
                pg_url,
                "-X",
                "-q",
                "-A",
                "-t",
                "-v",
                "ON_ERROR_STOP=1",
                "-c",
                sql,
            ],
            capture_output=True,
            text=True,
            timeout=120,
            check=False,
            env={
                **os.environ,
                "PGAPPNAME": "pg-parity-facts",
                "PGCONNECT_TIMEOUT": "10",
            },
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise FactsError(f"psql failed to run: {error}") from error
    if result.returncode != 0:
        raise FactsError(
            "psql export failed: " + (result.stderr.strip() or f"exit {result.returncode}")
        )
    payload = result.stdout.strip()
    try:
        return json.loads(payload)
    except json.JSONDecodeError as error:
        raise FactsError(f"psql did not return JSON: {error}: {payload[:200]}") from error


PG_SQL = r"""
SELECT json_build_object(
  'auth_total',
    (SELECT COUNT(*) FROM core.auth_token_logs),
  'auth_by_result',
    (SELECT COALESCE(json_agg(json_build_object('key', key, 'n', n) ORDER BY key), '[]'::json)
       FROM (
         SELECT COALESCE(NULLIF(TRIM(result_status), ''), 'unknown') AS key,
                COUNT(*) AS n
           FROM core.auth_token_logs
          GROUP BY 1
       ) s),
  'auth_by_kind',
    (SELECT COALESCE(json_agg(json_build_object('key', key, 'n', n) ORDER BY key), '[]'::json)
       FROM (
         SELECT COALESCE(NULLIF(TRIM(request_kind_key), ''), 'unknown') AS key,
                COUNT(*) AS n
           FROM core.auth_token_logs
          GROUP BY 1
       ) s),
  'logs_total',
    (SELECT COUNT(*) FROM observability.request_logs),
  'logs_by_visibility',
    (SELECT COALESCE(json_agg(json_build_object('key', key, 'n', n) ORDER BY key), '[]'::json)
       FROM (
         SELECT COALESCE(NULLIF(TRIM(visibility), ''), 'unknown') AS key,
                COUNT(*) AS n
           FROM observability.request_logs
          GROUP BY 1
       ) s),
  'bucket_days',
    (SELECT COALESCE(json_agg(json_build_object('day_key', day_key, 'total_requests', total_requests)
                              ORDER BY day_key), '[]'::json)
       FROM (
         SELECT to_char(to_timestamp(bucket_start::double precision) AT TIME ZONE 'UTC', 'YYYY-MM-DD')
                  AS day_key,
                SUM(total_requests) AS total_requests
           FROM observability.api_key_usage_buckets
          GROUP BY 1
       ) s),
  'rollup_days',
    (SELECT COALESCE(json_agg(json_build_object('day_key', day_key,
                                               'local_estimated_credits', local_estimated_credits)
                              ORDER BY day_key), '[]'::json)
       FROM (
         SELECT to_char(to_timestamp(bucket_start::double precision) AT TIME ZONE 'UTC', 'YYYY-MM-DD')
                  AS day_key,
                SUM(local_estimated_credits) AS local_estimated_credits
           FROM observability.dashboard_request_rollup_buckets
          WHERE bucket_secs = 86400
          GROUP BY 1
       ) s),
  'balance_total',
    (SELECT COALESCE(SUM(quota_remaining), 0)
       FROM core.api_keys
      WHERE deleted_at IS NULL),
  'alert_total',
    (SELECT COUNT(*) FROM observability.dashboard_alert_projection_events),
  'alert_rows',
    (SELECT COALESCE(json_agg(json_build_object('group_key', group_key, 'status', status, 'n', n)
                              ORDER BY group_key, status), '[]'::json)
       FROM (
         SELECT COALESCE((payload_json::json) ->> 'alert_type', 'unknown') AS group_key,
                COALESCE(
                    (payload_json::json) ->> 'result_status',
                    (payload_json::json) ->> 'job_status',
                    'unknown'
                ) AS status,
                COUNT(*) AS n
           FROM observability.dashboard_alert_projection_events
          GROUP BY 1, 2
       ) s),
  'invariant',
    (SELECT json_build_object(
       'bucket_total_requests',
         (SELECT COALESCE(SUM(total_requests), 0)
            FROM observability.api_key_usage_buckets),
       'request_logs_visible_with_key',
         (SELECT COUNT(*)
            FROM observability.request_logs
           WHERE visibility = 'visible' AND api_key_id IS NOT NULL),
       'window_start', w.window_start,
       'window_end', w.window_end,
       'window_secs', w.window_secs)
       FROM (
         SELECT MIN(bucket_start) AS window_start,
                MAX(bucket_start) + MAX(bucket_secs) AS window_end,
                MAX(bucket_secs) AS window_secs
           FROM observability.api_key_usage_buckets
       ) w)
)::text
"""


def _pg_raw(pg_url: str) -> dict[str, Any]:
    raw = _pg_json(pg_url, PG_SQL)
    if not isinstance(raw, dict):
        raise FactsError("postgres facts payload was not a JSON object")
    invariant = dict(raw["invariant"])
    window_start = invariant.get("window_start")
    window_end = invariant.get("window_end")
    if window_start is not None and window_end is not None:
        # The window-restricted count is the meaningful invariant when buckets exist.
        restricted = _pg_json(
            pg_url,
            f"""
            SELECT COUNT(*)::text
              FROM observability.request_logs
             WHERE visibility = 'visible'
               AND api_key_id IS NOT NULL
               AND created_at >= {int(window_start)}
               AND created_at < {int(window_end)}
            """,
        )
        invariant["request_logs_visible_with_key"] = int(restricted)
    raw["invariant"] = invariant
    return raw


# ---------------------------------------------------------------------------
# Canonical shape shared by both backends
# ---------------------------------------------------------------------------


def _cell(row: Any, field: str, index: int) -> Any:
    """Read a row that is either a JSON object (PostgreSQL) or a tuple (SQLite)."""
    if isinstance(row, dict):
        return row[field]
    return row[index]


def _count_map(rows: list[Any]) -> dict[str, int]:
    return {
        str(_cell(row, "key", 0)): int(_cell(row, "n", 1))
        for row in rows
    }


def _assemble(raw: dict[str, Any], backend: str, exported_at_unix: int) -> dict[str, Any]:
    bucket_days = {
        str(_cell(row, "day_key", 0)): int(_cell(row, "total_requests", 1))
        for row in raw["bucket_days"]
    }
    credit_days = {
        str(_cell(row, "day_key", 0)): int(_cell(row, "local_estimated_credits", 1))
        for row in raw["rollup_days"]
    }
    day_keys = sorted(set(bucket_days) | set(credit_days))
    alerts = [
        {
            "group_key": str(_cell(row, "group_key", 0)),
            "status": str(_cell(row, "status", 1)),
            "count": int(_cell(row, "n", 2)),
        }
        for row in raw["alert_rows"]
    ]
    invariant = dict(raw["invariant"])
    invariant["matched"] = (
        int(invariant["bucket_total_requests"])
        == int(invariant["request_logs_visible_with_key"])
    )
    return {
        "meta": {
            "backend": backend,
            "exported_at_unix": exported_at_unix,
            "day_key": "UTC date of bucket_start",
            "balance_source": "api_keys.quota_remaining",
            "alert_group_key_source": "dashboard_alert_projection_events.payload_json.alert_type",
        },
        "auth_token_logs": {
            "total": int(raw["auth_total"]),
            "by_result_status": _count_map(raw["auth_by_result"]),
            "by_request_kind_key": _count_map(raw["auth_by_kind"]),
        },
        "request_logs": {
            "total": int(raw["logs_total"]),
            "by_visibility": _count_map(raw["logs_by_visibility"]),
        },
        "api_key_usage_buckets": {
            "by_day": {
                day_key: {
                    "total_requests": bucket_days.get(day_key, 0),
                    "local_estimated_credits": credit_days.get(day_key, 0),
                }
                for day_key in day_keys
            }
        },
        "account_balance": {
            "total": int(raw["balance_total"]),
        },
        "alerts": {
            "total": int(raw["alert_total"]),
            "by_group_key_status": alerts,
        },
        "invariant": invariant,
    }


def export_sqlite(core_db_path: Path, output_path: Path) -> dict[str, Any]:
    import time

    facts = _assemble(_sqlite_raw(core_db_path), "sqlite", int(time.time()))
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(facts, indent=2, sort_keys=True) + "\n")
    return facts


def export_postgres(pg_url: str, output_path: Path) -> dict[str, Any]:
    import time

    facts = _assemble(_pg_raw(pg_url), "postgres", int(time.time()))
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(facts, indent=2, sort_keys=True) + "\n")
    return facts
