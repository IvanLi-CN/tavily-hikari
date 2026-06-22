import argparse
import sqlite3
import time

USER_PREFIX = "ha-fixture-user-"
TOKEN_PREFIX = "ha-fixture-token-"
SESSION_PREFIX = "ha-fixture-session-"
BILLING_LOG_ID_BASE = 9_000_000


def build_payload(prefix: str, size: int) -> str:
    if size <= 0:
        return ""
    seed = f"{prefix}-"
    repetitions = (size // len(seed)) + 1
    return (seed * repetitions)[:size]


def seed_control(conn: sqlite3.Connection, user_rows: int, token_rows: int, now: int) -> None:
    users = [
        (
            f"{USER_PREFIX}{idx:06d}",
            f"Fixture User {idx}",
            f"{USER_PREFIX}{idx:06d}",
            1,
            now,
            now,
        )
        for idx in range(user_rows)
    ]
    tokens = [
        (
            f"{TOKEN_PREFIX}{idx:06d}",
            f"fixture-secret-{idx:06d}",
            1,
            f"Fixture Token {idx}",
            "ha-memory",
            0,
            now,
            now,
        )
        for idx in range(token_rows)
    ]
    conn.executemany(
        """
        INSERT INTO users (
            id, display_name, username, active, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?)
        """,
        users,
    )
    conn.executemany(
        """
        INSERT INTO auth_tokens (
            id, secret, enabled, note, group_name, total_requests, created_at, last_used_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        """,
        tokens,
    )


def seed_runtime(
    conn: sqlite3.Connection,
    runtime_rows: int,
    runtime_text_bytes: int,
    token_rows: int,
    user_rows: int,
    now: int,
) -> None:
    payload = build_payload("runtime", runtime_text_bytes)
    sessions = [
        (
            f"{SESSION_PREFIX}{idx:06d}",
            f"upstream-session-{idx:06d}",
            f"{TOKEN_PREFIX}{idx % max(token_rows, 1):06d}",
            f"{USER_PREFIX}{idx % max(user_rows, 1):06d}",
            "2026-03-26",
            payload,
            payload,
            now,
            now,
            now + 86_400,
        )
        for idx in range(runtime_rows)
    ]
    conn.executemany(
        """
        INSERT INTO mcp_sessions (
            proxy_session_id,
            upstream_session_id,
            auth_token_id,
            user_id,
            protocol_version,
            routing_subject_hash,
            fallback_reason,
            created_at,
            updated_at,
            expires_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """,
        sessions,
    )


def seed_billing(
    conn: sqlite3.Connection,
    billing_rows: int,
    billing_error_bytes: int,
    token_rows: int,
    user_rows: int,
    now: int,
) -> None:
    payload = build_payload("billing", billing_error_bytes)
    rows = [
        (
            BILLING_LOG_ID_BASE + idx,
            f"{TOKEN_PREFIX}{idx % max(token_rows, 1):06d}",
            f"account:{USER_PREFIX}{idx % max(user_rows, 1):06d}",
            "charged",
            1,
            f"{USER_PREFIX}{idx % max(user_rows, 1):06d}",
            None,
            None,
            "success",
            now + idx,
            now + idx,
            payload,
        )
        for idx in range(billing_rows)
    ]
    conn.executemany(
        """
        INSERT INTO billing_ledger (
            auth_token_log_id,
            token_id,
            billing_subject,
            billing_state,
            business_credits,
            request_user_id,
            api_key_id,
            request_log_id,
            result_status,
            created_at,
            updated_at,
            error_message
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """,
        rows,
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--db", required=True)
    parser.add_argument("--user-rows", type=int, default=2000)
    parser.add_argument("--token-rows", type=int, default=2000)
    parser.add_argument("--runtime-rows", type=int, default=3000)
    parser.add_argument("--billing-rows", type=int, default=35000)
    parser.add_argument("--runtime-text-bytes", type=int, default=1024)
    parser.add_argument("--billing-error-bytes", type=int, default=4096)
    args = parser.parse_args()

    now = int(time.time())
    conn = sqlite3.connect(args.db)
    conn.execute("PRAGMA foreign_keys = OFF")
    conn.execute("PRAGMA journal_mode = WAL")
    conn.execute("PRAGMA synchronous = NORMAL")
    try:
        conn.execute("BEGIN IMMEDIATE")
        seed_control(conn, args.user_rows, args.token_rows, now)
        seed_runtime(
            conn,
            args.runtime_rows,
            args.runtime_text_bytes,
            args.token_rows,
            args.user_rows,
            now,
        )
        seed_billing(
            conn,
            args.billing_rows,
            args.billing_error_bytes,
            args.token_rows,
            args.user_rows,
            now,
        )
        conn.commit()
        conn.execute("PRAGMA wal_checkpoint(TRUNCATE)")
    finally:
        conn.close()

    print(
        {
            "db": args.db,
            "userRows": args.user_rows,
            "tokenRows": args.token_rows,
            "runtimeRows": args.runtime_rows,
            "billingRows": args.billing_rows,
            "runtimeTextBytes": args.runtime_text_bytes,
            "billingErrorBytes": args.billing_error_bytes,
        }
    )


if __name__ == "__main__":
    main()
