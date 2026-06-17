import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):
        print("[sidecar-upstream-mock]", fmt % args, flush=True)

    def _json(self, status, payload, headers=None):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        for key, value in (headers or {}).items():
            self.send_header(key, value)
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path.startswith("/usage"):
            self._json(200, {"key": {"limit": 100000, "usage": 1}})
            return
        if self.path.startswith("/research/"):
            self._json(200, {"status": "success", "usage": {"credits": 0}})
            return
        self._json(200, {"ok": True, "usage": {"credits": 1}})

    def do_POST(self):
        if self.path.startswith("/mcp"):
            self._json(
                200,
                {
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": {
                        "content": [{"type": "text", "text": "mock mcp ok"}],
                        "structuredContent": {"status": 200, "usage": {"credits": 1}},
                    },
                },
                {"mcp-session-id": "sidecar-upstream-mock-session"},
            )
            return
        self._json(
            200,
            {
                "answer": "mock tavily ok",
                "results": [{"title": "mock", "url": "https://example.test"}],
                "usage": {"credits": 1},
            },
        )


ThreadingHTTPServer(("0.0.0.0", 9001), Handler).serve_forever()
