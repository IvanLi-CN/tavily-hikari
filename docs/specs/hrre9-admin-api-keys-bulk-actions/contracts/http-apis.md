# HTTP API contracts

## `POST /api/keys/bulk-actions`

### Request

```json
{
  "action": "delete",
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
  - guard with the same admin-facing batch ceiling policy used for API keys bulk admin operations

### Response

```json
{
  "summary": {
    "requested": 3,
    "succeeded": 2,
    "skipped": 1,
    "failed": 0
  },
  "results": [
    {
      "key_id": "k1",
      "status": "success",
      "detail": null
    },
    {
      "key_id": "k2",
      "status": "skipped",
      "detail": "no active quarantine"
    },
    {
      "key_id": "k3",
      "status": "success",
      "detail": null
    }
  ]
}
```

### Semantics

- Legal request with mixed outcomes returns `200`; callers inspect `summary` + `results`.
- `summary.requested` equals the normalized unique `key_ids` count.
- `results` preserves execution order of normalized unique ids.
- `status` is one of:
  - `success`
  - `skipped`
  - `failed`
- `detail` is optional human-readable diagnostics for `skipped` / `failed`.

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
