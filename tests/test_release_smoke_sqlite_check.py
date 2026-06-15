#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import sqlite3
import sys
import tempfile
import unittest
from pathlib import Path


def load_module():
    script_path = (
        Path(__file__).resolve().parents[1]
        / ".github"
        / "scripts"
        / "release_smoke_sqlite_check.py"
    )
    spec = importlib.util.spec_from_file_location("release_smoke_sqlite_check", script_path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


MODULE = load_module()


REQUEST_LOGS_SCHEMA = """
CREATE TABLE request_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    auth_token_id TEXT,
    request_body BLOB,
    result_status TEXT,
    failure_kind TEXT,
    request_kind_key TEXT
)
"""

AUTH_TOKEN_LOGS_SCHEMA = """
CREATE TABLE auth_token_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    token_id TEXT,
    request_kind_key TEXT,
    result_status TEXT,
    business_credits INTEGER
)
"""

AUTH_TOKEN_QUOTA_SCHEMA = """
CREATE TABLE auth_token_quota (
    token_id TEXT PRIMARY KEY,
    month_count INTEGER
)
"""


def success_body(request_id: str = "release-smoke-search") -> bytes:
    return (
        '{"jsonrpc":"2.0","id":"%s","params":{"arguments":{"query":"release smoke gate"}}}'
        % request_id
    ).encode()


class ReleaseSmokeSqliteCheckTests(unittest.TestCase):
    def create_core_db(self, root: Path) -> Path:
        db_path = root / "tavily_proxy.db"
        conn = sqlite3.connect(db_path)
        conn.execute(AUTH_TOKEN_LOGS_SCHEMA)
        conn.execute(AUTH_TOKEN_QUOTA_SCHEMA)
        conn.commit()
        conn.close()
        return db_path

    def create_request_logs_db(self, path: Path) -> None:
        conn = sqlite3.connect(path)
        conn.execute(REQUEST_LOGS_SCHEMA)
        conn.commit()
        conn.close()

    def insert_smoke_rows(
        self,
        path: Path,
        token_id: str,
        *,
        request_id: str = "release-smoke-search",
        request_body: bytes | None = None,
    ) -> None:
        conn = sqlite3.connect(path)
        conn.execute(
            """
            INSERT INTO request_logs(auth_token_id, request_body, result_status, failure_kind, request_kind_key)
            VALUES (?, ?, 'success', NULL, 'mcp:search')
            """,
            (token_id, request_body or success_body(request_id)),
        )
        conn.execute(
            """
            INSERT INTO request_logs(auth_token_id, request_body, result_status, failure_kind, request_kind_key)
            VALUES (?, NULL, 'success', NULL, 'mcp:notifications/initialized')
            """,
            (token_id,),
        )
        conn.commit()
        conn.close()

    def insert_billing_rows(self, path: Path, token_id: str, charged_credits: int = 7) -> None:
        conn = sqlite3.connect(path)
        conn.execute(
            """
            INSERT INTO auth_token_logs(token_id, request_kind_key, result_status, business_credits)
            VALUES (?, 'mcp:search', 'success', ?)
            """,
            (token_id, charged_credits),
        )
        conn.execute(
            """
            INSERT INTO auth_token_quota(token_id, month_count)
            VALUES (?, ?)
            """,
            (token_id, charged_credits),
        )
        conn.commit()
        conn.close()

    def test_prefers_observability_sidecar_when_present(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            token_id = "token-sidecar"
            core_db = self.create_core_db(root)
            self.create_request_logs_db(root / "tavily_proxy-observability.db")
            self.create_request_logs_db(core_db)
            self.insert_smoke_rows(core_db, token_id, request_id="wrong-main-id")
            self.insert_smoke_rows(root / "tavily_proxy-observability.db", token_id)
            self.insert_billing_rows(core_db, token_id)

            conn = sqlite3.connect(core_db)
            try:
                request_logs_table = MODULE.resolve_request_logs_table(conn, core_db)
                self.assertEqual(request_logs_table, "observability.request_logs")
                MODULE.validate_request_logs(conn, request_logs_table, token_id)
                MODULE.validate_token_billing(conn, token_id)
            finally:
                conn.close()

    def test_falls_back_to_main_request_logs_for_legacy_single_db(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            token_id = "token-legacy"
            core_db = self.create_core_db(root)
            self.create_request_logs_db(core_db)
            self.insert_smoke_rows(core_db, token_id)
            self.insert_billing_rows(core_db, token_id)

            conn = sqlite3.connect(core_db)
            try:
                request_logs_table = MODULE.resolve_request_logs_table(conn, core_db)
                self.assertEqual(request_logs_table, "request_logs")
                MODULE.validate_request_logs(conn, request_logs_table, token_id)
                MODULE.validate_token_billing(conn, token_id)
            finally:
                conn.close()

    def test_main_and_observability_both_missing_raise_clear_error(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            core_db = self.create_core_db(root)

            conn = sqlite3.connect(core_db)
            try:
                with self.assertRaises(SystemExit) as ctx:
                    MODULE.resolve_request_logs_table(conn, core_db)
            finally:
                conn.close()

        self.assertIn("missing request_logs table", str(ctx.exception))

    def test_missing_cleaned_body_row_raises_validation_error(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            token_id = "token-no-cleaned"
            core_db = self.create_core_db(root)
            self.create_request_logs_db(core_db)

            conn = sqlite3.connect(core_db)
            conn.execute(
                """
                INSERT INTO request_logs(auth_token_id, request_body, result_status, failure_kind, request_kind_key)
                VALUES (?, ?, 'success', NULL, 'mcp:search')
                """,
                (token_id, success_body()),
            )
            conn.commit()
            conn.close()
            self.insert_billing_rows(core_db, token_id)

            conn = sqlite3.connect(core_db)
            try:
                with self.assertRaises(SystemExit) as ctx:
                    MODULE.validate_request_logs(conn, "request_logs", token_id)
            finally:
                conn.close()

        self.assertIn("cleaned or omitted body", str(ctx.exception))


if __name__ == "__main__":
    unittest.main()
