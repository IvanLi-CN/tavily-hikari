import json
import os
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

STATE_FILE = "/state/origin.txt"


def read_origin():
    if not os.path.exists(STATE_FILE):
        write_origin(os.environ.get("EDGEONE_INITIAL_ORIGIN", "node-a:8787"))
    with open(STATE_FILE, "r", encoding="utf-8") as handle:
        return handle.read().strip()


def write_origin(origin):
    os.makedirs(os.path.dirname(STATE_FILE), exist_ok=True)
    with open(STATE_FILE, "w", encoding="utf-8") as handle:
        handle.write(origin.strip())


def split_origin(origin):
    if ":" not in origin:
        return origin, 80
    host, port = origin.rsplit(":", 1)
    return host, int(port)


class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):
        print("[edgeone-mock]", fmt % args, flush=True)

    def _json(self, status, payload):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        parsed = urlparse(self.path)
        if parsed.path == "/origin":
            self._json(200, {"origin": read_origin()})
            return
        if parsed.path == "/set-origin":
            origin = parse_qs(parsed.query).get("origin", [""])[0]
            if not origin:
                self._json(400, {"error": "missing origin"})
                return
            write_origin(origin)
            self._json(200, {"origin": read_origin()})
            return
        self._json(404, {"error": "not found"})

    def do_POST(self):
        length = int(self.headers.get("content-length", "0"))
        raw = self.rfile.read(length) if length else b"{}"
        payload = json.loads(raw.decode("utf-8") or "{}")
        action = self.headers.get("x-tc-action", "")
        if action == "DescribeAccelerationDomains":
            host, port = split_origin(read_origin())
            self._json(
                200,
                {
                    "Response": {
                        "AccelerationDomains": [
                            {
                                "OriginDetail": {
                                    "Origin": host,
                                    "HttpOriginPort": port,
                                    "HttpsOriginPort": port,
                                }
                            }
                        ],
                        "RequestId": "describe-mock",
                    }
                },
            )
            return
        if action == "ModifyAccelerationDomain":
            info = payload.get("OriginInfo", {})
            host = info.get("Origin", "")
            port = int(payload.get("HttpsOriginPort") or payload.get("HttpOriginPort") or 443)
            write_origin(f"{host}:{port}")
            self._json(200, {"Response": {"RequestId": "modify-mock", "Origin": read_origin()}})
            return
        self._json(400, {"Response": {"Error": {"Message": f"unknown action {action}"}}})


ThreadingHTTPServer(("0.0.0.0", 9000), Handler).serve_forever()
