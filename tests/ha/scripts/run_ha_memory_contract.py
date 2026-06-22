import argparse
import json
import os
import sqlite3
import time
import urllib.error
import urllib.request

INGRESS = os.environ.get("INGRESS_URL", "http://edgeone-ingress:8080")
NODE_A = os.environ.get("NODE_A_URL", "http://node-a:8787")
NODE_B = os.environ.get("NODE_B_URL", "http://node-b:8787")
STANDBY_DB = os.environ.get("STANDBY_DB_PATH", "/volumes/node-b/node-b.db")

USER_PREFIX = "ha-fixture-user-"
TOKEN_PREFIX = "ha-fixture-token-"
SESSION_PREFIX = "ha-fixture-session-"
BILLING_LOG_ID_BASE = 9_000_000


def request(method, url, body=None, headers=None, timeout=10, parse_json=True):
    data = None
    req_headers = dict(headers or {})
    if body is not None:
        data = json.dumps(body).encode("utf-8")
        req_headers["content-type"] = "application/json"
    req = urllib.request.Request(url, data=data, method=method, headers=req_headers)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as response:
            raw = response.read()
            parsed = None
            if raw and parse_json:
                try:
                    parsed = json.loads(raw.decode("utf-8"))
                except (json.JSONDecodeError, UnicodeDecodeError):
                    parsed = raw.decode("utf-8", errors="replace")
            return response.status, parsed, dict(response.headers), raw
    except urllib.error.HTTPError as err:
        raw = err.read()
        parsed = None
        if raw and parse_json:
            try:
                parsed = json.loads(raw.decode("utf-8"))
            except (json.JSONDecodeError, UnicodeDecodeError):
                parsed = raw.decode("utf-8", errors="replace")
        return err.code, parsed, dict(err.headers), raw


def read_counts(db_path):
    conn = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True, timeout=1)
    try:
        cursor = conn.cursor()
        counts = {}
        counts["users"] = cursor.execute(
            "SELECT COUNT(*) FROM users WHERE id LIKE ?", (f"{USER_PREFIX}%",)
        ).fetchone()[0]
        counts["tokens"] = cursor.execute(
            "SELECT COUNT(*) FROM auth_tokens WHERE id LIKE ?", (f"{TOKEN_PREFIX}%",)
        ).fetchone()[0]
        counts["sessions"] = cursor.execute(
            "SELECT COUNT(*) FROM mcp_sessions WHERE proxy_session_id LIKE ?",
            (f"{SESSION_PREFIX}%",),
        ).fetchone()[0]
        counts["billing"] = cursor.execute(
            "SELECT COUNT(*) FROM billing_ledger WHERE auth_token_log_id >= ?",
            (BILLING_LOG_ID_BASE,),
        ).fetchone()[0]
        return counts
    finally:
        conn.close()


def wait_for_sync(expected, timeout, settle_seconds):
    deadline = time.time() + timeout
    last = {}
    stable_since = None
    while time.time() < deadline:
        status, body, _headers, _raw = request("GET", f"{NODE_B}/api/ha/status")
        counts = read_counts(STANDBY_DB)
        last = {
            "status": status,
            "body": body,
            "counts": counts,
        }
        ready = (
            status == 200
            and body["role"] == "standby"
            and body.get("lastSyncAt") is not None
            and counts["users"] >= expected["users"]
            and counts["tokens"] >= expected["tokens"]
            and counts["sessions"] >= expected["sessions"]
            and counts["billing"] >= expected["billing"]
        )
        if ready:
            if stable_since is None:
                stable_since = time.time()
            if time.time() - stable_since >= settle_seconds:
                return last
        else:
            stable_since = None
        time.sleep(1)
    raise AssertionError(f"timed out waiting for standby sync: {json.dumps(last, ensure_ascii=False)}")


def stress_billing_export(expected_rows, repetitions, ha_token):
    results = []
    for idx in range(repetitions):
        status, _body, headers, raw = request(
            "GET",
            f"{NODE_A}/api/admin/ha/baseline?channel=billing",
            headers={"x-ha-internal-token": ha_token},
            timeout=120,
            parse_json=False,
        )
        if status != 200:
            raise AssertionError(f"billing baseline export {idx} failed with HTTP {status}")
        row_count = int(headers.get("x-ha-row-count", "0"))
        if row_count < expected_rows:
            raise AssertionError(
                f"billing baseline export {idx} row count {row_count} < expected {expected_rows}"
            )
        results.append(
            {
                "attempt": idx + 1,
                "rowCount": row_count,
                "compressedBytes": len(raw),
                "highWatermark": int(headers.get("x-ha-high-watermark", "0")),
            }
        )
    return results


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--expected-users", type=int, required=True)
    parser.add_argument("--expected-tokens", type=int, required=True)
    parser.add_argument("--expected-sessions", type=int, required=True)
    parser.add_argument("--expected-billing", type=int, required=True)
    parser.add_argument("--billing-export-repetitions", type=int, default=5)
    parser.add_argument("--ha-internal-token", required=True)
    parser.add_argument("--sync-timeout-seconds", type=int, default=180)
    parser.add_argument("--settle-seconds", type=int, default=5)
    args = parser.parse_args()

    expected = {
        "users": args.expected_users,
        "tokens": args.expected_tokens,
        "sessions": args.expected_sessions,
        "billing": args.expected_billing,
    }

    synced = wait_for_sync(expected, args.sync_timeout_seconds, args.settle_seconds)
    exports = stress_billing_export(
        args.expected_billing,
        args.billing_export_repetitions,
        args.ha_internal_token,
    )
    print(
        json.dumps(
            {
                "standbyStatus": synced["body"],
                "standbyCounts": synced["counts"],
                "billingExports": exports,
                "nodeA": NODE_A,
                "nodeB": NODE_B,
                "ingress": INGRESS,
            },
            ensure_ascii=False,
        )
    )


if __name__ == "__main__":
    main()
