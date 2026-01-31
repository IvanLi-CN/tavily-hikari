#!/usr/bin/env python3
import base64
import os
from http.server import BaseHTTPRequestHandler, HTTPServer


def _expected_basic_token() -> str:
    user = os.environ.get("AUTH_USER", "admin")
    password = os.environ.get("AUTH_PASS", "password")
    raw = f"{user}:{password}".encode("utf-8")
    return base64.b64encode(raw).decode("ascii")


EXPECTED = _expected_basic_token()
REALM = os.environ.get("AUTH_REALM", "auth-mock")


class Handler(BaseHTTPRequestHandler):
    server_version = "auth-mock/1.0"

    def log_message(self, fmt: str, *args) -> None:
        print(f"{self.address_string()} - {fmt % args}")

    def _unauthorized(self) -> None:
        self.send_response(401)
        self.send_header("WWW-Authenticate", f'Basic realm="{REALM}"')
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.end_headers()
        self.wfile.write(b"Unauthorized\n")

    def _ok(self) -> None:
        self.send_response(200)
        self.send_header("Remote-Email", "admin@example.com")
        self.send_header("Remote-Name", "admin")
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.end_headers()
        self.wfile.write(b"ok\n")

    def do_GET(self) -> None:
        if self.path != "/auth":
            self.send_response(404)
            self.send_header("Content-Type", "text/plain; charset=utf-8")
            self.end_headers()
            self.wfile.write(b"Not Found\n")
            return

        auth = self.headers.get("Authorization", "")
        if not auth.startswith("Basic "):
            self._unauthorized()
            return

        token = auth.removeprefix("Basic ").strip()
        if token != EXPECTED:
            self._unauthorized()
            return

        self._ok()


def main() -> None:
    host = os.environ.get("HOST", "0.0.0.0")
    port = int(os.environ.get("PORT", "8080"))
    httpd = HTTPServer((host, port), Handler)
    print(f"auth-mock listening on http://{host}:{port}")
    httpd.serve_forever()


if __name__ == "__main__":
    main()

