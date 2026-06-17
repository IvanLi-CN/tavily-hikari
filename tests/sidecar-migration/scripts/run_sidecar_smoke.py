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
MIGRATION_REPORT = "/srv/app/runtime/migrate-report.json"


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


def parse_sse_json_message(value):
    if isinstance(value, dict):
        return value
    if not isinstance(value, str):
        raise AssertionError(f"unexpected response body type: {type(value)!r}")
    for line in value.splitlines():
        if not line.startswith("data:"):
            continue
        data = line.removeprefix("data:").strip()
        if data:
            return json.loads(data)
    raise AssertionError(f"missing SSE data message: {value[:200]}")


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


def load_migration_report():
    if not os.path.exists(MIGRATION_REPORT):
        return None
    with open(MIGRATION_REPORT, "r", encoding="utf-8") as fh:
        return json.load(fh)


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
    body = parse_sse_json_message(body)
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

    report = load_migration_report()
    sidecar_count, sidecar_min_id, sidecar_max_id = sqlite_rows(
        SIDECAR_DB,
        "SELECT COUNT(*), MIN(id), MAX(id) FROM request_logs",
    )[0]
    if report:
        source_rows = int(report.get("sourceRequestLogRows") or 0)
        source_min = report.get("sourceMinRequestLogId")
        source_max = report.get("sourceMaxRequestLogId")
        if sidecar_count < source_rows + 2:
            raise AssertionError(
                f"expected migrated history plus fresh rows, got {sidecar_count} < {source_rows + 2}"
            )
        if source_min is not None and sidecar_min_id != source_min:
            raise AssertionError(f"sidecar min id changed: {sidecar_min_id} != {source_min}")
        if source_max is not None and sidecar_max_id < source_max:
            raise AssertionError(f"sidecar max id regressed: {sidecar_max_id} < {source_max}")
    elif sidecar_count < 4:
        raise AssertionError(f"expected migrated plus smoke rows, got {sidecar_count}")

    sidecar_path_counts = dict(
        sqlite_rows(
            SIDECAR_DB,
            """
            SELECT path, COUNT(*)
            FROM request_logs
            WHERE path IN ('/api/tavily/search', '/mcp')
            GROUP BY path
            """,
        )
    )
    if sidecar_path_counts.get("/api/tavily/search", 0) < 1 or sidecar_path_counts.get("/mcp", 0) < 1:
        raise AssertionError(f"missing expected sidecar log paths: {sidecar_path_counts}")

    payload = {
        "backend": version["backend"],
        "tokenId": token_id,
        "logPaths": sorted(paths),
        "sidecarRowCount": sidecar_count,
        "sidecarMinId": sidecar_min_id,
        "sidecarMaxId": sidecar_max_id,
    }
    print(json.dumps(payload))


if __name__ == "__main__":
    main()
