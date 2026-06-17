import json
import os
import sqlite3
import time
import urllib.parse
import urllib.error
import urllib.request


APP_URL = os.environ.get("APP_URL", "http://app:8787")
CORE_DB = "/srv/app/runtime/data/tavily_proxy.db"
SIDECAR_DB = "/srv/app/runtime/data/tavily_proxy-observability.db"


def request(method, url, body=None, headers=None, timeout=15):
    data = None
    req_headers = dict(headers or {})
    if body is not None:
        data = json.dumps(body).encode("utf-8")
        req_headers["content-type"] = "application/json"
    req = urllib.request.Request(url, data=data, method=method, headers=req_headers)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as response:
            raw = response.read().decode("utf-8")
            try:
                parsed = json.loads(raw) if raw else None
            except json.JSONDecodeError:
                parsed = raw
            return response.status, parsed
    except urllib.error.HTTPError as err:
        raw = err.read().decode("utf-8")
        try:
            parsed = json.loads(raw) if raw else None
        except json.JSONDecodeError:
            parsed = raw
        return err.code, parsed


def wait_ok(path, timeout=90):
    deadline = time.time() + timeout
    last = None
    while time.time() < deadline:
        try:
            status, body = request("GET", f"{APP_URL}{path}")
            last = (status, body)
            if status == 200:
                return body
        except Exception as exc:  # noqa: BLE001
            last = repr(exc)
        time.sleep(1)
    raise AssertionError(f"timed out waiting for {path}; last={last}")


def assert_status(label, actual, expected):
    if actual != expected:
        raise AssertionError(f"{label}: expected HTTP {expected}, got {actual}")


def create_token():
    note = f"sidecar smoke {int(time.time() * 1000)}"
    status, body = request("POST", f"{APP_URL}/api/tokens", {"note": note})
    assert_status("create token", status, 201)
    token = body["token"]

    status, listing = request(
        "GET",
        f"{APP_URL}/api/tokens?page=1&per_page=20&q={urllib.parse.quote(note)}",
    )
    assert_status("list token", status, 200)
    items = listing.get("items") or []
    token_id = next((item.get("id") for item in items if item.get("note") == note), None)
    if not token_id:
        raise AssertionError(f"failed to resolve token id for note={note!r}: {listing}")
    return token, token_id


def sqlite_scalar(path, sql, params=()):
    with sqlite3.connect(path) as conn:
        row = conn.execute(sql, params).fetchone()
    return None if row is None else row[0]


def sqlite_rows(path, sql, params=()):
    with sqlite3.connect(path) as conn:
        rows = conn.execute(sql, params).fetchall()
    return rows


def main():
    wait_ok("/health")
    version = wait_ok("/api/version")
    if not version.get("backend"):
        raise AssertionError(f"missing backend version: {version}")

    token, token_id = create_token()
    auth = {"authorization": f"Bearer {token}"}

    status, body = request(
        "POST",
        f"{APP_URL}/api/tavily/search",
        {"query": "sidecar smoke"},
        headers=auth,
    )
    assert_status("search", status, 200)
    if not isinstance(body, dict) or not isinstance(body.get("results"), list):
        raise AssertionError(f"unexpected search body: {body}")

    status, body = request(
        "POST",
        f"{APP_URL}/mcp?tavilyApiKey={token}",
        {"jsonrpc": "2.0", "id": 1, "method": "tools/list"},
        timeout=20,
    )
    assert_status("mcp tools/list", status, 200)
    if body.get("jsonrpc") != "2.0":
        raise AssertionError(f"unexpected mcp body: {body}")

    logs = wait_ok("/api/logs?page=1&per_page=20")
    items = logs.get("items") or []
    if len(items) < 2:
        raise AssertionError(f"expected migrated + fresh request logs, got {len(items)}")
    paths = {item.get("path") for item in items}
    if "/api/tavily/search" not in paths or "/mcp" not in paths:
        raise AssertionError(f"missing expected log paths: {paths}")

    token_logs = wait_ok(f"/api/tokens/{token_id}/logs/page?page=1&per_page=20&since=0")
    token_items = token_logs.get("items") or []
    if not token_items:
        raise AssertionError("expected token log page to include fresh entries")

    main_request_logs_exists = sqlite_scalar(
        CORE_DB,
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'request_logs' LIMIT 1",
    )
    if main_request_logs_exists is not None:
        raise AssertionError("core DB still owns main.request_logs after migration")

    sidecar_rows = sqlite_rows(
        SIDECAR_DB,
        "SELECT id, path FROM request_logs ORDER BY id ASC",
    )
    if len(sidecar_rows) < 4:
        raise AssertionError(f"expected migrated plus smoke rows, got {len(sidecar_rows)}")
    if sidecar_rows[0][0] != 1 or sidecar_rows[0][1] != "/api/tavily/search":
        raise AssertionError(f"unexpected seed row 1: {sidecar_rows[0]}")
    if sidecar_rows[1][0] != 2 or sidecar_rows[1][1] != "/mcp":
        raise AssertionError(f"unexpected seed row 2: {sidecar_rows[1]}")

    payload = {
        "backend": version["backend"],
        "tokenId": token_id,
        "logPaths": sorted(paths),
        "sidecarRowCount": len(sidecar_rows),
    }
    print(json.dumps(payload))


if __name__ == "__main__":
    main()
