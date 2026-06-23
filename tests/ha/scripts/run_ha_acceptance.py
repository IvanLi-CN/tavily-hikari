import argparse
import concurrent.futures
import json
import os
import time
import urllib.error
import urllib.request

INGRESS = os.environ.get("INGRESS_URL", "http://edgeone-ingress:8080")
EDGEONE = os.environ.get("EDGEONE_MOCK_URL", "http://edgeone-mock:9000")
NODE_A = os.environ.get("NODE_A_URL", "http://node-a:8787")
NODE_B = os.environ.get("NODE_B_URL", "http://node-b:8787")
STATE_FILE = "/tmp/ha_acceptance_state.json"


def request(method, url, body=None, token=None, headers=None, timeout=10):
    data = None
    req_headers = dict(headers or {})
    if body is not None:
        data = json.dumps(body).encode("utf-8")
        req_headers["content-type"] = "application/json"
    if token:
        req_headers["authorization"] = f"Bearer {token}"
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


def wait_json(url, predicate, label, timeout=90):
    deadline = time.time() + timeout
    last = None
    while time.time() < deadline:
        try:
            status, body = request("GET", url)
            last = (status, body)
            if status == 200 and predicate(body):
                return body
        except Exception as exc:  # noqa: BLE001
            last = repr(exc)
        time.sleep(1)
    raise AssertionError(f"timed out waiting for {label}; last={last}")


def assert_status(label, actual, expected):
    if actual != expected:
        raise AssertionError(f"{label}: expected HTTP {expected}, got {actual}")


def create_token(base):
    status, body = request("POST", f"{base}/api/tokens", {"note": "ha acceptance"})
    assert_status("create token", status, 201)
    return body["token"]


def write_state(**values):
    current = {}
    if os.path.exists(STATE_FILE):
        with open(STATE_FILE, "r", encoding="utf-8") as handle:
            current = json.load(handle)
    current.update(values)
    with open(STATE_FILE, "w", encoding="utf-8") as handle:
        json.dump(current, handle)


def read_state(key):
    with open(STATE_FILE, "r", encoding="utf-8") as handle:
        return json.load(handle)[key]


def active_origin():
    status, body = request("GET", f"{EDGEONE}/origin")
    assert_status("edgeone origin", status, 200)
    return body["origin"]


def stage_pre():
    wait_json(f"{NODE_A}/health", lambda _: True, "node-a health")
    wait_json(f"{NODE_B}/health", lambda _: True, "node-b health")
    node_a = wait_json(
        f"{NODE_A}/api/admin/ha/status",
        lambda body: body["role"] == "full_master",
        "node-a full_master",
    )
    node_b = wait_json(
        f"{NODE_B}/api/admin/ha/status",
        lambda body: body["role"] == "standby",
        "node-b standby",
    )
    assert node_a["edgeoneOrigin"] == "node-a:8787", node_a
    assert node_b["edgeoneOrigin"] == "node-a:8787", node_b
    assert active_origin() == "node-a:8787"

    token = create_token(INGRESS)
    status, _ = request("POST", f"{INGRESS}/api/tavily/search", {"query": "ha"}, token)
    assert_status("ingress tavily search", status, 200)
    status, _ = request(
        "POST",
        f"{INGRESS}/mcp",
        {"jsonrpc": "2.0", "id": 1, "method": "tools/list"},
        token,
    )
    assert_status("ingress mcp", status, 200)

    status, body = request("POST", f"{NODE_B}/api/tavily/search", {"query": "blocked"})
    assert_status("standby tavily business gate", status, 503)
    assert body["role"] == "standby", body
    status, _ = request("POST", f"{NODE_B}/mcp", {"jsonrpc": "2.0", "id": 1, "method": "tools/list"})
    assert_status("standby mcp business gate", status, 503)
    status, _ = request("POST", f"{NODE_B}/api/tokens", {"note": "blocked"})
    assert_status("standby token write gate", status, 503)

    sentinel = f"tvly-ha-sentinel-{int(time.time())}"
    status, body = request("POST", f"{NODE_A}/api/keys", {"api_key": sentinel})
    assert_status("create sentinel key on active", status, 201)
    sentinel_id = body["id"]

    def has_sentinel(_body):
        status, raw = request("GET", f"{NODE_B}/api/keys/{sentinel_id}/secret")
        return status == 200 and raw.get("api_key") == sentinel

    wait_json(f"{NODE_B}/api/admin/ha/status", has_sentinel, "standby state sync", timeout=30)
    write_state(token=token, sentinel=sentinel)
    print(json.dumps({"stage": "pre", "token": token, "sentinel": sentinel}))


def stage_failover():
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
        cutover = executor.submit(
            request,
            "POST",
            f"{NODE_A}/api/admin/ha/planned-cutover",
            {"targetNodeId": "node-b"},
        )
        active_rejected = executor.submit(request, "POST", f"{NODE_A}/api/admin/ha/promote", {})
        b_status, b_body = cutover.result(timeout=20)
        a_status, _a_body = active_rejected.result(timeout=20)
    assert_status("planned cutover to node-b", b_status, 200)
    assert b_body["status"] == "success", b_body
    assert_status("active node-a non-force promote rejected", a_status, 409)
    assert active_origin() == "node-b:8787"
    node_a = wait_json(
        f"{NODE_A}/api/admin/ha/status",
        lambda body: body["role"] == "recovery",
        "node-a recovery after EdgeOne origin switch",
        timeout=30,
    )
    assert node_a["allowsBasicBusiness"] is False, node_a

    token = read_state("token")
    status, _ = request("POST", f"{INGRESS}/api/tavily/search", {"query": "after failover"}, token)
    assert_status("provisional ingress tavily search", status, 200)
    status, _ = request(
        "POST",
        f"{INGRESS}/mcp",
        {"jsonrpc": "2.0", "id": 1, "method": "tools/list"},
        token,
    )
    assert_status("provisional ingress mcp", status, 200)
    node_b = wait_json(
        f"{NODE_B}/api/admin/ha/status",
        lambda body: body["role"] == "full_master",
        "node-b full_master after planned cutover",
        timeout=30,
    )
    assert node_b["role"] == "full_master", node_b
    status, _ = request("POST", f"{NODE_B}/api/tokens", {"note": "after finalize"})
    assert_status("full master token write restored", status, 201)
    print(json.dumps({"stage": "failover", "origin": active_origin()}))


def stage_recovery():
    node_a = wait_json(
        f"{NODE_A}/api/admin/ha/status",
        lambda body: body["role"] == "recovery",
        "node-a recovery",
        timeout=60,
    )
    assert node_a["recoveryStatus"], node_a
    before_status, before_settings = request("GET", f"{NODE_B}/api/settings")
    assert_status("read settings before recovery", before_status, 200)
    forbidden_payload = {
        "batchId": "old-node-a-batch-1",
        "sourceNodeId": "node-a",
        "message": "node-a forbidden log recovery batch",
        "requestLogs": [
            {
                "authTokenId": "old-node-a-token",
                "method": "POST",
                "path": "/api/tavily/search",
                "statusCode": 200,
                "tavilyStatusCode": 200,
                "resultStatus": "success",
                "requestKindKey": "tavily_search",
                "requestKindLabel": "Tavily Search",
                "businessCredits": 1,
                "requestBody": "{\"query\":\"old\"}",
                "responseBody": "{\"ok\":true}",
                "forwardedHeaders": "[]",
                "droppedHeaders": "[]",
                "visibility": "visible",
                "createdAt": int(time.time()) - 30,
            }
        ],
        "authTokenLogs": [
            {
                "tokenId": "old-node-a-token",
                "method": "POST",
                "path": "/api/tavily/search",
                "httpStatus": 200,
                "mcpStatus": 200,
                "requestKindKey": "tavily_search",
                "requestKindLabel": "Tavily Search",
                "resultStatus": "success",
                "countsBusinessQuota": 1,
                "businessCredits": 1,
                "billingState": "charged",
                "createdAt": int(time.time()) - 30,
            }
        ],
    }
    status, body = request("POST", f"{NODE_B}/api/admin/ha/recovery/import", forbidden_payload)
    assert_status("recovery log import rejected", status, 400)
    assert "request_logs" in str(body) and "auth_token_logs" in str(body), body

    payload = {
        "batchId": "old-node-a-batch-1",
        "sourceNodeId": "node-a",
        "message": "node-a ledger recovery batch",
    }
    status, body = request("POST", f"{NODE_B}/api/admin/ha/recovery/import", payload)
    assert_status("recovery import first", status, 200)
    assert body["imported"] is True, body
    assert body["eventCount"] == 0, body
    assert body["status"]["role"] == "full_master", body
    status, body = request("POST", f"{NODE_B}/api/admin/ha/recovery/import", payload)
    assert_status("recovery import duplicate", status, 200)
    assert body["imported"] is False, body
    after_status, after_settings = request("GET", f"{NODE_B}/api/settings")
    assert_status("read settings after recovery", after_status, 200)
    assert before_settings == after_settings
    print(json.dumps({"stage": "recovery", "nodeA": node_a["role"]}))


parser = argparse.ArgumentParser()
parser.add_argument("stage", choices=["pre", "failover", "recovery"])
args = parser.parse_args()
globals()[f"stage_{args.stage}"]()
