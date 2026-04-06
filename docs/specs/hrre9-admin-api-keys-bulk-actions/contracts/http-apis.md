# HTTP API contracts

## `POST /api/keys/bulk-actions`

### Request

```json
{
  "action": "sync_usage",
  "key_ids": ["k1", "k2", "k3"]
}
```

### Fields

- `action: "delete" | "clear_quarantine" | "sync_usage"`
- `key_ids: string[]`
  - required
  - trim each item
  - drop blanks
  - de-duplicate before execution
  - empty after normalization => `400`
  - guarded by the same admin batch ceiling used elsewhere in the API Keys admin surface

### JSON response

All bulk actions continue to support the existing JSON response. `delete` and `clear_quarantine` are JSON-only. `sync_usage` returns this JSON shape when the caller does not ask for SSE.

```json
{
  "summary": {
    "requested": 3,
    "succeeded": 2,
    "skipped": 0,
    "failed": 1
  },
  "results": [
    {
      "key_id": "k1",
      "status": "success",
      "detail": null
    },
    {
      "key_id": "k2",
      "status": "success",
      "detail": null
    },
    {
      "key_id": "k3",
      "status": "failed",
      "detail": "Tavily usage request failed with 401: {\"error\":\"unauthorized\"}"
    }
  ]
}
```

### JSON semantics

- Legal request with mixed outcomes returns `200`; callers inspect `summary` + `results`.
- `summary.requested` equals the normalized unique `key_ids` count.
- `results` preserves execution order of normalized unique ids.
- `status` is one of:
  - `success`
  - `skipped`
  - `failed`
- `detail` is optional human-readable diagnostics for `skipped` / `failed`.

### SSE negotiation for `sync_usage`

When both conditions are true:

- `action === "sync_usage"`
- request `Accept` header contains `text/event-stream`

then the same endpoint upgrades to an SSE response with `Content-Type: text/event-stream`. The server emits JSON payloads in `data:` frames.

`delete` and `clear_quarantine` never upgrade to SSE; they remain JSON-only even when the caller advertises `text/event-stream`.

### SSE event shapes

#### `phase`

Emitted for coarse-grained phases that the UI can render directly.

```json
{
  "type": "phase",
  "phaseKey": "prepare_request",
  "label": "Preparing request",
  "detail": "Queued 3 key(s) for manual quota sync",
  "current": 0,
  "total": 3
}
```

```json
{
  "type": "phase",
  "phaseKey": "refresh_ui",
  "label": "Refreshing list",
  "detail": "Server-side sync finished; refresh the admin keys list now",
  "current": 3,
  "total": 3
}
```

Rules:

- `phaseKey` is one of `prepare_request | refresh_ui`.
- `prepare_request` is emitted before any per-key work begins.
- `refresh_ui` is emitted after the last key result is known and before the terminal `complete` event.

#### `item`

Emitted once per processed key in normalized execution order.

```json
{
  "type": "item",
  "keyId": "k2",
  "status": "failed",
  "current": 2,
  "total": 3,
  "summary": {
    "requested": 3,
    "succeeded": 1,
    "skipped": 0,
    "failed": 1
  },
  "detail": "Tavily usage request failed with 401: {\"error\":\"unauthorized\"}"
}
```

Rules:

- `current` is the number of processed keys so far.
- `summary` is cumulative at the moment of emission.
- `status` uses the same domain as JSON results: `success | skipped | failed`.
- `detail` is optional and normally only present for skipped/failed outcomes.

#### `complete`

Terminal success event.

```json
{
  "type": "complete",
  "payload": {
    "summary": {
      "requested": 3,
      "succeeded": 2,
      "skipped": 0,
      "failed": 1
    },
    "results": [
      { "key_id": "k1", "status": "success", "detail": null },
      { "key_id": "k2", "status": "success", "detail": null },
      {
        "key_id": "k3",
        "status": "failed",
        "detail": "Tavily usage request failed with 401: {\"error\":\"unauthorized\"}"
      }
    ]
  }
}
```

Rules:

- `complete` is terminal.
- `payload.summary` and `payload.results` are contract-equivalent to the normal JSON response path.

#### `error`

Terminal transport/encoding failure event.

```json
{
  "type": "error",
  "message": "failed to encode bulk sync item event",
  "phaseKey": "sync_usage",
  "detail": "..."
}
```

Rules:

- `error` is terminal.
- `phaseKey` is optional and indicates the phase where the stream failed.
- Per-key upstream failures are **not** emitted as stream-level `error`; they are represented as `item.status = "failed"`.

### Ordering guarantees for `sync_usage` SSE

For a successful stream, the order is:

1. one `phase(prepare_request)`
2. zero or more `item` events, exactly one per normalized key id
3. one `phase(refresh_ui)`
4. one terminal `complete`

If the stream itself fails to encode/emit, the server may terminate early with one terminal `error`.

### Action-specific rules

- `delete`
  - Uses existing soft-delete semantics.
  - Success does not remove historical undelete compatibility.
- `clear_quarantine`
  - Key without active quarantine => `skipped`.
  - Key with active quarantine cleared => `success`.
- `sync_usage`
  - Must allow manual execution regardless of local key status (`active`, `disabled`, `exhausted`, `quarantined`).
  - Per-key execution reuses existing manual quota sync behavior and job logging.
  - Upstream failure must not persist incorrect `quota_limit`, `quota_remaining`, `quota_synced_at`, or quota sync sample rows for that key.
  - Existing upstream-failure quarantine/audit side effects remain unchanged.

## `POST /api/keys/:id/sync-usage`

### Response shape

- Unchanged.

### Semantics

- Manual admin sync is allowed regardless of local key status.
- Button disabling is only tied to in-flight request state.
- Upstream failure must not persist incorrect `quota_limit`, `quota_remaining`, `quota_synced_at`, or quota sync sample rows.
- Existing upstream-failure quarantine/audit side effects remain unchanged.
