#!/usr/bin/env python3
import json
import os
from http.server import BaseHTTPRequestHandler, HTTPServer


def _send_json(handler: BaseHTTPRequestHandler, status: int, body: dict) -> None:
    data = json.dumps(body, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
    handler.send_response(status)
    handler.send_header("Content-Type", "application/json; charset=utf-8")
    handler.send_header("Content-Length", str(len(data)))
    handler.end_headers()
    handler.wfile.write(data)


class Handler(BaseHTTPRequestHandler):
    server_version = "upstream-mock/1.0"

    def log_message(self, fmt: str, *args) -> None:
        print(f"{self.address_string()} - {fmt % args}")

    def do_GET(self) -> None:
        if self.path == "/health":
            self.send_response(200)
            self.send_header("Content-Type", "text/plain; charset=utf-8")
            self.end_headers()
            self.wfile.write(b"ok\n")
            return

        if self.path == "/usage":
            _send_json(
                self,
                200,
                {
                    "key": {"limit": 1000, "usage": 0},
                    "account": {"plan_limit": 1000, "plan_usage": 0},
                },
            )
            return

        _send_json(self, 404, {"error": "not_found", "path": self.path})

    def do_POST(self) -> None:
        if self.path.startswith("/mcp"):
            length = int(self.headers.get("Content-Length", "0"))
            raw = self.rfile.read(length) if length > 0 else b""
            _send_json(
                self,
                200,
                {
                    "ok": True,
                    "mock": "tavily-upstream",
                    "path": self.path,
                    "method": "POST",
                    "body_bytes": len(raw),
                },
            )
            return

        _send_json(self, 404, {"error": "not_found", "path": self.path})


def main() -> None:
    host = os.environ.get("HOST", "0.0.0.0")
    port = int(os.environ.get("PORT", "8080"))
    httpd = HTTPServer((host, port), Handler)
    print(f"upstream-mock listening on http://{host}:{port}")
    httpd.serve_forever()


if __name__ == "__main__":
    main()

