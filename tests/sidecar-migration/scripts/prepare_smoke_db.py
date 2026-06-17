import os
import sqlite3
import time


CORE_DB = "/srv/app/runtime/data/tavily_proxy.db"


def table_exists(conn, name):
    row = conn.execute(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ? LIMIT 1",
        (name,),
    ).fetchone()
    return row is not None


def main():
    now = int(time.time())
    os.makedirs(os.path.dirname(CORE_DB), exist_ok=True)
    with sqlite3.connect(CORE_DB) as conn:
        if table_exists(conn, "forward_proxy_settings"):
            conn.execute(
                """
                UPDATE forward_proxy_settings
                SET proxy_urls_json = '[]',
                    subscription_urls_json = '[]',
                    insert_direct = 1,
                    egress_socks5_enabled = 0,
                    egress_socks5_url = '',
                    updated_at = ?
                """,
                (now,),
            )
        for table in (
            "forward_proxy_runtime",
            "forward_proxy_node_overrides",
            "forward_proxy_attempts",
            "forward_proxy_key_affinity",
            "forward_proxy_weight_hourly",
        ):
            if table_exists(conn, table):
                conn.execute(f"DELETE FROM {table}")
        conn.commit()
    print("prepared sidecar smoke database")


if __name__ == "__main__":
    main()
