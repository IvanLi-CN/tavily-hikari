#!/usr/bin/env python3
from __future__ import annotations

import json
import sqlite3
import sys
from pathlib import Path


def sqlite_sidecar_path(database_path: Path, file_name: str) -> Path:
    stem = database_path.stem or "sqlite"
    if "." in file_name:
        base, ext = file_name.rsplit(".", 1)
        sidecar_name = f"{stem}-{base}.{ext}"
    else:
        sidecar_name = f"{stem}-{file_name}"
    return database_path.parent / sidecar_name


def candidate_request_log_sources(db_path: Path) -> list[tuple[str, str]]:
    sidecar_path = sqlite_sidecar_path(db_path, "observability.db")
    return [
        ("observability", str(sidecar_path)),
        ("main", ""),
    ]


def attach_observability_if_present(conn: sqlite3.Connection, sidecar_path: str) -> None:
    if not Path(sidecar_path).exists():
        return
    conn.execute("ATTACH DATABASE ? AS observability", (sidecar_path,))


def table_exists(conn: sqlite3.Connection, schema: str, table: str) -> bool:
    if schema == "main":
        row = conn.execute(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ? LIMIT 1",
            (table,),
        ).fetchone()
        return row is not None
    row = conn.execute(
        f"SELECT 1 FROM {schema}.sqlite_master WHERE type = 'table' AND name = ? LIMIT 1",
        (table,),
    ).fetchone()
    return row is not None


def resolve_request_logs_table(conn: sqlite3.Connection, db_path: Path) -> str:
    attach_observability_if_present(conn, str(sqlite_sidecar_path(db_path, "observability.db")))
    for schema, _ in candidate_request_log_sources(db_path):
        if schema == "observability":
            try:
                if table_exists(conn, schema, "request_logs"):
                    return "observability.request_logs"
            except sqlite3.OperationalError:
                continue
        elif table_exists(conn, schema, "request_logs"):
            return "request_logs"
    raise SystemExit(
        "missing request_logs table in both main and observability SQLite layouts"
    )


def validate_request_logs(
    conn: sqlite3.Connection,
    request_logs_table: str,
    token_id: str,
) -> None:
    request_rows = conn.execute(
        f"""
        SELECT request_body, result_status, failure_kind, request_kind_key
        FROM {request_logs_table}
        WHERE auth_token_id = ?
        ORDER BY id DESC
        """,
        (token_id,),
    ).fetchall()
    if not request_rows:
        raise SystemExit("missing request_logs row for smoke token")

    success_search_rows = []
    cleaned_body_rows = 0
    for request_body_raw, result_status, failure_kind, request_kind_key in request_rows:
        if request_body_raw is None:
            cleaned_body_rows += 1
            continue
        request_body = json.loads(bytes(request_body_raw).decode())
        request_id = request_body.get("id")
        if request_id == "release-smoke-search":
            success_search_rows.append((request_body, result_status, failure_kind))

    if cleaned_body_rows < 1:
        raise SystemExit(
            "expected at least one request_logs row with a cleaned or omitted body under release smoke retention policy"
        )

    if len(success_search_rows) != 1:
        raise SystemExit(
            f"expected exactly one successful MCP search request_logs row for smoke token, got {len(success_search_rows)}"
        )

    request_body, result_status, failure_kind = success_search_rows[0]
    if request_body["params"]["arguments"].get("include_usage") is not None:
        raise SystemExit(f"include_usage must not be forwarded for MCP smoke: {request_body}")
    if result_status != "success" or failure_kind is not None:
        raise SystemExit(
            f"unexpected request_logs outcome for MCP smoke search: result={result_status} failure={failure_kind}"
        )


def validate_token_billing(conn: sqlite3.Connection, token_id: str) -> None:
    token_log_row = conn.execute(
        "SELECT business_credits FROM auth_token_logs WHERE token_id = ? AND request_kind_key = 'mcp:search' AND result_status = 'success' ORDER BY id DESC LIMIT 1",
        (token_id,),
    ).fetchone()
    if token_log_row is None or token_log_row[0] is None or token_log_row[0] <= 0:
        raise SystemExit(f"missing charged credits for smoke token: {token_log_row}")

    month_row = conn.execute(
        "SELECT COALESCE(month_count, 0) FROM auth_token_quota WHERE token_id = ? LIMIT 1",
        (token_id,),
    ).fetchone()
    if month_row is None or month_row[0] < token_log_row[0]:
        raise SystemExit(
            f"token monthly quota did not increase with billed credits: month_row={month_row} charged={token_log_row}"
        )


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        raise SystemExit("usage: release_smoke_sqlite_check.py <db_path> <token_id>")

    db_path = Path(argv[1])
    token_id = argv[2]
    conn = sqlite3.connect(db_path)
    try:
        request_logs_table = resolve_request_logs_table(conn, db_path)
        validate_request_logs(conn, request_logs_table, token_id)
        validate_token_billing(conn, token_id)
    finally:
        conn.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
