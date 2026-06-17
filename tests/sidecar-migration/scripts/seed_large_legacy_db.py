import os
import sqlite3
import sys


THRESHOLD = 32 * 1024 * 1024


def exec_many(conn, statements):
    for sql in statements:
        conn.execute(sql)


def main():
    if len(sys.argv) != 2:
        raise SystemExit("usage: seed_large_legacy_db.py <db-path>")
    db_path = sys.argv[1]
    os.makedirs(os.path.dirname(db_path), exist_ok=True)
    if os.path.exists(db_path):
        os.remove(db_path)

    conn = sqlite3.connect(db_path)
    exec_many(
        conn,
        [
            """
            CREATE TABLE api_keys (
                id TEXT PRIMARY KEY,
                api_key TEXT NOT NULL UNIQUE,
                created_at INTEGER NOT NULL DEFAULT 0,
                last_used_at INTEGER NOT NULL DEFAULT 0
            )
            """,
            """
            CREATE TABLE request_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                api_key_id TEXT,
                method TEXT NOT NULL,
                path TEXT NOT NULL,
                result_status TEXT NOT NULL DEFAULT 'success',
                visibility TEXT NOT NULL DEFAULT 'visible',
                created_at INTEGER NOT NULL,
                FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
            )
            """,
            """
            CREATE TABLE billing_ledger (
                auth_token_log_id INTEGER PRIMARY KEY,
                token_id TEXT NOT NULL,
                billing_subject TEXT,
                billing_state TEXT NOT NULL DEFAULT 'none',
                business_credits INTEGER,
                request_user_id TEXT,
                api_key_id TEXT,
                request_log_id INTEGER,
                result_status TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                settled_at INTEGER,
                error_message TEXT
            )
            """,
            """
            CREATE TABLE api_key_maintenance_records (
                id TEXT PRIMARY KEY,
                key_id TEXT NOT NULL,
                source TEXT NOT NULL,
                operation_code TEXT NOT NULL,
                operation_summary TEXT NOT NULL,
                reason_code TEXT,
                reason_summary TEXT,
                reason_detail TEXT,
                request_log_id INTEGER,
                auth_token_log_id INTEGER,
                auth_token_id TEXT,
                actor_user_id TEXT,
                actor_display_name TEXT,
                status_before TEXT,
                status_after TEXT,
                quarantine_before INTEGER NOT NULL DEFAULT 0,
                quarantine_after INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL
            )
            """,
            """
            CREATE TABLE api_key_transient_backoffs (
                key_id TEXT NOT NULL,
                scope TEXT NOT NULL,
                cooldown_until INTEGER NOT NULL,
                retry_after_secs INTEGER NOT NULL,
                reason_code TEXT,
                source_request_log_id INTEGER,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (key_id, scope)
            )
            """,
            """
            CREATE TABLE api_key_usage_buckets (
                api_key_id TEXT NOT NULL,
                bucket_start INTEGER NOT NULL,
                bucket_secs INTEGER NOT NULL,
                total_requests INTEGER NOT NULL DEFAULT 0,
                success_count INTEGER NOT NULL DEFAULT 0,
                error_count INTEGER NOT NULL DEFAULT 0,
                quota_exhausted_count INTEGER NOT NULL DEFAULT 0,
                valuable_success_count INTEGER NOT NULL DEFAULT 0,
                valuable_failure_count INTEGER NOT NULL DEFAULT 0,
                other_success_count INTEGER NOT NULL DEFAULT 0,
                other_failure_count INTEGER NOT NULL DEFAULT 0,
                unknown_count INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (api_key_id, bucket_start, bucket_secs)
            )
            """,
            """
            CREATE TABLE dashboard_request_rollup_buckets (
                bucket_start INTEGER PRIMARY KEY,
                total_requests INTEGER NOT NULL DEFAULT 0,
                success_count INTEGER NOT NULL DEFAULT 0,
                error_count INTEGER NOT NULL DEFAULT 0,
                quota_exhausted_count INTEGER NOT NULL DEFAULT 0,
                valuable_success_count INTEGER NOT NULL DEFAULT 0,
                valuable_failure_count INTEGER NOT NULL DEFAULT 0,
                valuable_failure_429_count INTEGER NOT NULL DEFAULT 0,
                other_success_count INTEGER NOT NULL DEFAULT 0,
                other_failure_count INTEGER NOT NULL DEFAULT 0,
                unknown_count INTEGER NOT NULL DEFAULT 0,
                mcp_non_billable INTEGER NOT NULL DEFAULT 0,
                mcp_billable INTEGER NOT NULL DEFAULT 0,
                api_non_billable INTEGER NOT NULL DEFAULT 0,
                api_billable INTEGER NOT NULL DEFAULT 0,
                local_estimated_credits INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL DEFAULT 0
            )
            """,
            """
            CREATE TABLE request_log_catalog_rollups (
                bucket_start INTEGER NOT NULL,
                request_kind_key TEXT NOT NULL,
                request_kind_label TEXT NOT NULL,
                result_bucket TEXT NOT NULL,
                key_effect_code TEXT NOT NULL,
                binding_effect_code TEXT NOT NULL,
                selection_effect_code TEXT NOT NULL,
                auth_token_id TEXT NOT NULL DEFAULT '',
                api_key_id TEXT NOT NULL DEFAULT '',
                operational_class TEXT NOT NULL,
                request_count INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (
                    bucket_start,
                    request_kind_key,
                    request_kind_label,
                    result_bucket,
                    key_effect_code,
                    binding_effect_code,
                    selection_effect_code,
                    auth_token_id,
                    api_key_id,
                    operational_class
                )
            )
            """,
            """
            CREATE TABLE meta (
                key TEXT PRIMARY KEY,
                value TEXT
            )
            """,
        ],
    )

    conn.execute(
        "INSERT INTO api_keys (id, api_key, created_at, last_used_at) VALUES ('k1', 'tvly-sidecar-migration-seed', 1, 1)"
    )
    for row_id, path, ts in [
        (1, "/api/tavily/search", 1710000001),
        (2, "/mcp", 1710000002),
    ]:
        conn.execute(
            """
            INSERT INTO request_logs (id, api_key_id, method, path, result_status, visibility, created_at)
            VALUES (?, 'k1', 'POST', ?, 'success', 'visible', ?)
            """,
            (row_id, path, ts),
        )
    conn.execute(
        """
        INSERT INTO billing_ledger (
            auth_token_log_id,
            token_id,
            billing_subject,
            billing_state,
            business_credits,
            api_key_id,
            request_log_id,
            result_status,
            created_at
        ) VALUES (1, 'seed-token', 'account:seed-user', 'charged', 1, 'k1', 1, 'success', 1710000001)
        """
    )
    conn.execute(
        """
        INSERT INTO api_key_maintenance_records (
            id,
            key_id,
            source,
            operation_code,
            operation_summary,
            request_log_id,
            created_at
        ) VALUES ('seed-maint', 'k1', 'auto', 'noop', 'noop', 2, 1710000002)
        """
    )
    conn.execute(
        """
        INSERT INTO api_key_transient_backoffs (
            key_id,
            scope,
            cooldown_until,
            retry_after_secs,
            source_request_log_id,
            created_at,
            updated_at
        ) VALUES ('k1', 'seed-scope', 1710003600, 60, 2, 1710000002, 1710000002)
        """
    )
    conn.execute(
        """
        INSERT INTO api_key_usage_buckets (
            api_key_id,
            bucket_start,
            bucket_secs,
            total_requests,
            success_count,
            updated_at
        ) VALUES ('k1', 1710000000, 86400, 2, 2, 1710000002)
        """
    )
    conn.execute(
        """
        INSERT INTO dashboard_request_rollup_buckets (
            bucket_start,
            total_requests,
            success_count,
            mcp_billable,
            api_billable,
            local_estimated_credits,
            updated_at
        ) VALUES (1710000000, 2, 2, 1, 1, 2, 1710000002)
        """
    )
    conn.execute(
        """
        INSERT INTO request_log_catalog_rollups (
            bucket_start,
            request_kind_key,
            request_kind_label,
            result_bucket,
            key_effect_code,
            binding_effect_code,
            selection_effect_code,
            auth_token_id,
            api_key_id,
            operational_class,
            request_count,
            updated_at
        ) VALUES (
            1710000000,
            'api:search',
            'API | search',
            'success',
            'none',
            'none',
            'none',
            '',
            'k1',
            'success',
            1,
            1710000001
        )
        """
    )
    conn.execute(
        """
        INSERT INTO request_log_catalog_rollups (
            bucket_start,
            request_kind_key,
            request_kind_label,
            result_bucket,
            key_effect_code,
            binding_effect_code,
            selection_effect_code,
            auth_token_id,
            api_key_id,
            operational_class,
            request_count,
            updated_at
        ) VALUES (
            1710000000,
            'mcp:tools/list',
            'MCP | tools/list',
            'success',
            'none',
            'none',
            'none',
            '',
            'k1',
            'success',
            1,
            1710000002
        )
        """
    )
    for key, value in [
        ("api_key_usage_buckets_v1_done", "1"),
        ("api_key_usage_buckets_request_value_v2_done", "1"),
        ("dashboard_request_rollup_buckets_v1_done", "1"),
        ("request_log_catalog_rollup_v1_done", "1"),
        ("request_log_catalog_rollup_v1_retention_days", "32"),
    ]:
        conn.execute("INSERT INTO meta (key, value) VALUES (?, ?)", (key, value))

    conn.commit()
    conn.close()

    with open(db_path, "r+b") as handle:
        handle.truncate(THRESHOLD + 4096)


if __name__ == "__main__":
    main()
