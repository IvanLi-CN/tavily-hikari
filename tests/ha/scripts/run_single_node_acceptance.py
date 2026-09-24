import json
import os
import time
import urllib.error
import urllib.request


NODE_URL = os.environ.get("NODE_URL", "http://node-a:8787")


def request(path, *, parse_json=True):
    try:
        with urllib.request.urlopen(f"{NODE_URL}{path}", timeout=5) as response:
            body = response.read().decode("utf-8")
            return response.status, json.loads(body) if parse_json else None
    except urllib.error.HTTPError as error:
        body = error.read().decode("utf-8")
        raise AssertionError(f"{path}: HTTP {error.code}: {body}") from error


def wait_for_health():
    deadline = time.time() + 300
    while time.time() < deadline:
        try:
            status, _body = request("/health", parse_json=False)
            if status == 200:
                return
        except (AssertionError, OSError):
            pass
        time.sleep(1)
    raise AssertionError("timed out waiting for single-node health")


def assert_single_node_status(path):
    status, body = request(path)
    assert status == 200, (path, status, body)
    assert body["mode"] == "single", body
    assert body["role"] == "full_master", body
    assert body["allowsFullWrites"] is True, body
    assert body["peerCount"] == 0, body
    assert body["syncDisabledReason"] is None, body
    assert body["peerNodes"] == [], body
    return body


wait_for_health()
admin_status = assert_single_node_status("/api/admin/ha/status")
public_status = assert_single_node_status("/api/ha/status")
print(json.dumps({"mode": admin_status["mode"], "admin": admin_status, "public": public_status}))
