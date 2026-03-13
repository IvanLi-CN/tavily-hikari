# HTTP contracts

## `GET /api/keys`

### Query

- `page?: number`
  - default: `1`
  - normalized to integer `>= 1`
- `per_page?: number`
  - default: `20`
  - normalized to integer clamp `1..100`
- `group?: string[]`
  - repeatable query key
  - `group=` (blank after trim) means the ungrouped bucket
  - exact match against normalized `group_name`
- `status?: string[]`
  - repeatable query key
  - blank values ignored after trim
  - lowercase exact match against list badge status (`active`, `disabled`, `quarantined`, ...)

### Response

```json
{
  "items": [
    {
      "id": "9fbd",
      "status": "active",
      "group": "2026-03-12",
      "status_changed_at": 1741835543,
      "last_used_at": 1741835576,
      "deleted_at": null,
      "quota_limit": 1000,
      "quota_remaining": 831,
      "quota_synced_at": 1741835500,
      "total_requests": 20297,
      "success_count": 20297,
      "error_count": 421,
      "quota_exhausted_count": 0,
      "quarantine": null
    }
  ],
  "total": 37,
  "page": 2,
  "perPage": 20,
  "facets": {
    "groups": [
      { "value": "2026-03-12", "count": 12 },
      { "value": "", "count": 9 }
    ],
    "statuses": [
      { "value": "active", "count": 28 },
      { "value": "quarantined", "count": 9 }
    ]
  }
}
```

### Semantics

- `items` is the paged slice after all filters are applied.
- `total` is the filtered total count before paging.
- `page` and `perPage` are the normalized effective values used by the server.
- `facets.groups` and `facets.statuses` must be suitable for rendering filter menus without requiring a second list request.
- Soft-deleted keys are excluded from both `items` and facet counts.

## Browser route contract: `/admin/keys`

- UI query keys:
  - `page`
  - `perPage`
  - repeated `group`
  - repeated `status`
- Default values omitted from URL:
  - omit `page` when value is `1`
  - omit `perPage` when value is `20`
  - omit `group` / `status` when empty
- Entering `/admin/keys/:id` from the list preserves the originating query string for the detail back-link.
