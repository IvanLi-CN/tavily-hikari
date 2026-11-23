#!/usr/bin/env python3
"""Deterministic traffic for the SQLite/PostgreSQL differential replay.

The plan is derived only from the seed and the requested duration: the request
sequence, the payloads, the concurrency groups and the pacing are all decided
before the first request is sent. Wall-clock time only paces the plan; it never
changes what is sent.

Every request records `(seq, route, method, rpc_method, status)`. `seq` is
assigned at plan time, so the exported JSON is byte-identical for one seed even
though a concurrency group finishes in an arbitrary order.

The MCP lane runs the local rebalance facade. The facade serves
`initialize`/`tools/list` locally and forwards `tools/call` to the HTTP stub via
`--usage-base /search`, which is the only MCP shape the checked-in stub can
serve end to end.
"""

from __future__ import annotations

import argparse
import http.client
import json
import random
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import Any
from urllib.parse import urlsplit


CYCLE_SECS = 0.5
SEARCH_QUERIES = (
    "parity alpha",
    "parity bravo",
    "parity charlie",
    "parity delta",
    "parity echo",
    "parity foxtrot",
    "parity golf",
    "parity hotel",
)
DASHBOARD_ROUTES = (
    "/api/dashboard/overview",
    "/api/logs",
    "/api/users/rankings",
)
MCP_PROTOCOL_VERSION = "2025-03-26"


class TrafficError(RuntimeError):
    """Raised when the traffic run cannot be completed as specified."""


def _split(base_url: str) -> tuple[str, int]:
    parsed = urlsplit(base_url)
    if parsed.scheme != "http":
        raise TrafficError(f"only http:// base URLs are supported: {base_url}")
    if not parsed.hostname:
        raise TrafficError(f"base URL has no host: {base_url}")
    return parsed.hostname, parsed.port or 80


def _request(
    base_url: str,
    method: str,
    path: str,
    *,
    body: dict[str, Any] | None = None,
    headers: dict[str, str] | None = None,
    timeout: float = 15.0,
) -> tuple[int, dict[str, str], bytes]:
    host, port = _split(base_url)
    payload = None if body is None else json.dumps(body).encode()
    request_headers = {"Accept": "application/json, text/event-stream"}
    if payload is not None:
        request_headers["Content-Type"] = "application/json"
    request_headers.update(headers or {})
    connection = http.client.HTTPConnection(host, port, timeout=timeout)
    try:
        connection.request(method, path, body=payload, headers=request_headers)
        response = connection.getresponse()
        data = response.read()
        response_headers = {
            key.lower(): value for key, value in response.getheaders()
        }
        return response.status, response_headers, data
    finally:
        connection.close()


def configure_gateway(base_url: str) -> None:
    """Pin the MCP gateway switch so both backends start from the same settings."""
    status, _headers, data = _request(
        base_url,
        "PUT",
        "/api/settings/system",
        body={
            "mcpSessionAffinityKeyCount": 1,
            "rebalanceMcpEnabled": True,
            "rebalanceMcpSessionPercent": 100,
            "apiRebalanceEnabled": False,
        },
    )
    if status != 200:
        raise TrafficError(
            "failed to pin system settings: "
            f"status={status} body={data[:200].decode(errors='replace')}"
        )


def create_token(base_url: str, note: str) -> tuple[int, str | None]:
    status, _headers, data = _request(
        base_url, "POST", "/api/tokens", body={"note": note}
    )
    token = None
    if status == 201:
        try:
            token = json.loads(data).get("token")
        except json.JSONDecodeError:
            token = None
    return status, token


def _mcp_call(
    base_url: str,
    token: str,
    message: dict[str, Any],
    *,
    session_id: str | None = None,
    protocol_version: str | None = None,
) -> tuple[int, dict[str, str], bytes]:
    headers = {"Authorization": f"Bearer {token}"}
    if session_id:
        headers["mcp-session-id"] = session_id
    if protocol_version:
        headers["mcp-protocol-version"] = protocol_version
    return _request(base_url, "POST", "/mcp", body=message, headers=headers)


def initialize_session(
    base_url: str, token: str, label: str
) -> tuple[int, str | None, str | None]:
    status, headers, _data = _mcp_call(
        base_url,
        token,
        {
            "jsonrpc": "2.0",
            "id": f"init-{label}",
            "method": "initialize",
            "params": {
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "pg-parity", "version": "1.0.0"},
            },
        },
    )
    return (
        status,
        headers.get("mcp-session-id"),
        headers.get("mcp-protocol-version") or MCP_PROTOCOL_VERSION,
    )


def _tools_list_message(seq: int) -> dict[str, Any]:
    return {"jsonrpc": "2.0", "id": f"list-{seq}", "method": "tools/list", "params": {}}


def _tools_call_message(seq: int, query: str) -> dict[str, Any]:
    return {
        "jsonrpc": "2.0",
        "id": f"call-{seq}",
        "method": "tools/call",
        "params": {
            "name": "tavily_search",
            "arguments": {
                "query": query,
                "search_depth": "basic",
                "max_results": 1,
            },
        },
    }


def build_plan(seed: int, duration_secs: int, start_seq: int = 0) -> list[dict[str, Any]]:
    """Build the full request plan before any request is sent."""
    rng = random.Random(seed)
    cycles = max(1, int(duration_secs / CYCLE_SECS))
    plan: list[dict[str, Any]] = []
    next_seq = start_seq
    for cycle in range(cycles):
        batch: list[dict[str, Any]] = []

        query = f"{rng.choice(SEARCH_QUERIES)} {cycle}"
        batch.append(
            {
                "kind": "search",
                "seq": next_seq,
                "query": query,
            }
        )
        next_seq += 1

        dashboard_route = DASHBOARD_ROUTES[cycle % len(DASHBOARD_ROUTES)]
        batch.append(
            {
                "kind": "dashboard",
                "seq": next_seq,
                "route": dashboard_route,
            }
        )
        next_seq += 1

        if cycle % 20 == 0:
            # A new session exercises initialize + tools/list + tools/call in order.
            seqs = {"initialize": next_seq, "list": next_seq + 1, "call": next_seq + 2}
            next_seq += 3
            batch.append(
                {
                    "kind": "new_session",
                    "seqs": seqs,
                    "label": f"cycle-{cycle}",
                    "query": f"mcp session {cycle} {rng.choice(SEARCH_QUERIES)}",
                }
            )
        else:
            if cycle % 10 == 0:
                batch.append({"kind": "mcp_list", "seq": next_seq})
                next_seq += 1
            if cycle % 3 == 0:
                batch.append(
                    {
                        "kind": "mcp_call",
                        "seq": next_seq,
                        "query": f"mcp call {cycle} {rng.choice(SEARCH_QUERIES)}",
                    }
                )
                next_seq += 1

        if cycle % 15 == 0:
            batch.append({"kind": "create_token", "seq": next_seq, "note": f"parity {cycle}"})
            next_seq += 1

        plan.append({"cycle": cycle, "gap_secs": CYCLE_SECS, "requests": batch})
    return plan


def _record(
    results: dict[int, dict[str, Any]],
    seq: int,
    route: str,
    method: str,
    *,
    rpc_method: str | None = None,
    status: int | None,
    error: str | None = None,
) -> None:
    entry: dict[str, Any] = {
        "seq": seq,
        "route": route,
        "method": method,
        "status": status,
    }
    if rpc_method:
        entry["rpc_method"] = rpc_method
    if error:
        entry["error"] = error
    results[seq] = entry


def _execute_request(
    base_url: str,
    token: str,
    session: dict[str, str | None],
    item: dict[str, Any],
    results: dict[int, dict[str, Any]],
) -> None:
    kind = item["kind"]
    seq = item.get("seq", -1)
    if kind == "search":
        status, _headers, _data = _request(
            base_url,
            "POST",
            "/api/tavily/search",
            body={
                "query": item["query"],
                "search_depth": "basic",
                "max_results": 1,
            },
            headers={"Authorization": f"Bearer {token}"},
        )
        _record(results, seq, "/api/tavily/search", "POST", status=status)
    elif kind == "dashboard":
        status, _headers, _data = _request(base_url, "GET", item["route"])
        _record(results, seq, item["route"], "GET", status=status)
    elif kind == "mcp_list":
        status, _headers, _data = _mcp_call(
            base_url,
            token,
            _tools_list_message(seq),
            session_id=session.get("id"),
            protocol_version=session.get("protocol"),
        )
        _record(
            results, seq, "/mcp", "POST", rpc_method="tools/list", status=status
        )
    elif kind == "mcp_call":
        status, _headers, _data = _mcp_call(
            base_url,
            token,
            _tools_call_message(seq, item["query"]),
            session_id=session.get("id"),
            protocol_version=session.get("protocol"),
        )
        _record(
            results, seq, "/mcp", "POST", rpc_method="tools/call", status=status
        )
    elif kind == "create_token":
        status, _token = create_token(base_url, item["note"])
        _record(results, seq, "/api/tokens", "POST", status=status)
    elif kind == "new_session":
        seqs = item["seqs"]
        status, session_id, protocol = initialize_session(
            base_url, token, item["label"]
        )
        _record(
            results,
            seqs["initialize"],
            "/mcp",
            "POST",
            rpc_method="initialize",
            status=status,
        )
        if session_id:
            session["id"] = session_id
            session["protocol"] = protocol
        status, _headers, _data = _mcp_call(
            base_url,
            token,
            _tools_list_message(seqs["list"]),
            session_id=session.get("id"),
            protocol_version=session.get("protocol"),
        )
        _record(
            results,
            seqs["list"],
            "/mcp",
            "POST",
            rpc_method="tools/list",
            status=status,
        )
        status, _headers, _data = _mcp_call(
            base_url,
            token,
            _tools_call_message(seqs["call"], item["query"]),
            session_id=session.get("id"),
            protocol_version=session.get("protocol"),
        )
        _record(
            results,
            seqs["call"],
            "/mcp",
            "POST",
            rpc_method="tools/call",
            status=status,
        )
    else:  # pragma: no cover - plan construction owns the vocabulary
        raise TrafficError(f"unknown plan item kind: {kind}")


def run(
    base_url: str,
    *,
    seed: int,
    duration_secs: int,
    output_path: Path,
) -> dict[str, Any]:
    configure_gateway(base_url)

    results: dict[int, dict[str, Any]] = {}
    next_seq = 0

    token_status, token = create_token(base_url, "parity bootstrap")
    _record(results, next_seq, "/api/tokens", "POST", status=token_status)
    next_seq += 1
    if token_status != 201 or not token:
        raise TrafficError(
            f"bootstrap token creation failed: status={token_status}"
        )

    init_status, session_id, protocol = initialize_session(
        base_url, token, "bootstrap"
    )
    _record(
        results,
        next_seq,
        "/mcp",
        "POST",
        rpc_method="initialize",
        status=init_status,
    )
    next_seq += 1
    if not session_id:
        raise TrafficError(
            "rebalance MCP initialize did not return mcp-session-id; "
            "cannot drive tools/list or tools/call"
        )
    session: dict[str, str | None] = {"id": session_id, "protocol": protocol}

    plan = build_plan(seed, duration_secs, start_seq=next_seq)
    with ThreadPoolExecutor(max_workers=4) as pool:
        for cycle in plan:
            futures = [
                pool.submit(_execute_request, base_url, token, session, item, results)
                for item in cycle["requests"]
            ]
            for future in futures:
                future.result()
            time.sleep(cycle["gap_secs"])

    requests = [results[seq] for seq in sorted(results)]
    summary: dict[str, int] = {}
    for entry in requests:
        key = f"{entry['method']} {entry['route']}"
        if entry.get("rpc_method"):
            key += f"#{entry['rpc_method']}"
        key += f" -> {entry['status']}"
        summary[key] = summary.get(key, 0) + 1

    payload = {
        "meta": {
            "seed": seed,
            "duration_secs": duration_secs,
            "cycle_secs": CYCLE_SECS,
            "gateway_mode": "rebalance",
            "planned_requests": len(results),
        },
        "requests": requests,
        "summary": dict(sorted(summary.items())),
    }
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")
    return payload


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Generate the deterministic pg-parity traffic against a running backend."
    )
    parser.add_argument("--base-url", default="http://127.0.0.1:58087")
    parser.add_argument("--seed", type=int, default=20261123)
    parser.add_argument("--duration-secs", type=int, default=30)
    parser.add_argument("--output", type=Path, default=Path("traffic.json"))
    args = parser.parse_args(argv)
    payload = run(
        args.base_url,
        seed=args.seed,
        duration_secs=args.duration_secs,
        output_path=args.output,
    )
    print(json.dumps(payload["summary"], indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
