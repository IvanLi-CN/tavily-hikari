#!/usr/bin/env python3
"""Small internal-only Tavily protocol stub for snapshot comparisons."""

from __future__ import annotations

import argparse
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Handler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        if self.path == "/usage":
            self.respond(
                200,
                {
                    "key": {"limit": 1_000_000, "usage": 0, "research_usage": 0},
                    "account": {"plan_limit": 1_000_000, "plan_usage": 0},
                },
            )
            return
        if self.path.startswith("/research/"):
            self.respond(200, {"status": "completed"})
            return
        self.respond(404, {"error": "not found"})

    def do_POST(self) -> None:
        length = int(self.headers.get("Content-Length", "0"))
        raw_body = self.rfile.read(length)
        try:
            body = json.loads(raw_body or b"{}")
        except json.JSONDecodeError:
            self.respond(400, {"error": "invalid json"})
            return

        if self.path == "/search":
            self.respond(
                200,
                {
                    "query": body.get("query", ""),
                    "results": [],
                    "answer": None,
                    "images": [],
                    "response_time": 0.01,
                    "status": 200,
                    "request_id": "performance-recovery-search",
                    "usage": {"credits": 1},
                },
            )
            return
        self.respond(200, {"status": 200, "usage": {"credits": 1}})

    def respond(self, status: int, body: dict[str, object]) -> None:
        encoded = json.dumps(body, separators=(",", ":")).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)

    def log_message(self, _format: str, *_args: object) -> None:
        pass


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--bind", default="0.0.0.0")
    parser.add_argument("--port", type=int, default=9001)
    args = parser.parse_args()
    ThreadingHTTPServer((args.bind, args.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
