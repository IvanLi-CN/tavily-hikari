# HTTP APIs

## `GET /api/users`

- Added response fields on each item:
  - `monthlyBrokenCount: number`
  - `monthlyBrokenLimit: number`
- Added sort field:
  - `sort=monthlyBrokenCount`
- Ordering semantics:
  - server sorts by `monthlyBrokenCount`
  - ties sort by `monthlyBrokenLimit`
  - final tie-breaker `userId ASC`

## `GET /api/users/:id`

- Added response field:
  - `monthlyBrokenLimit: number`

## `PATCH /api/users/:id/broken-key-limit`

Request:

```json
{
  "monthlyBrokenLimit": 7
}
```

Response:

- `204 No Content`

## `GET /api/users/:id/broken-keys?page=&per_page=`

Response:

```json
{
  "items": [
    {
      "keyId": "key_prod_a",
      "currentStatus": "quarantined",
      "reasonCode": "manual_quarantine",
      "reasonSummary": "确认该 Key 被上游封禁",
      "latestBreakAt": 1774502400,
      "source": "manual",
      "breakerTokenId": "9vsN",
      "breakerUserId": "usr_alice",
      "breakerUserDisplayName": "Alice Wang",
      "manualActorDisplayName": null,
      "relatedUsers": [
        {
          "userId": "usr_alice",
          "displayName": "Alice Wang",
          "username": "alice"
        }
      ]
    }
  ],
  "total": 1,
  "page": 1,
  "perPage": 20
}
```

## `GET /api/tokens/unbound-usage`

- Added response fields on each item:
  - `monthlyBrokenCount: number | null`
  - `monthlyBrokenLimit: number | null`
- Added query support:
  - `sort=monthlyBrokenCount`
  - server sorts rows with current-month records first
  - ties sort by `monthlyBrokenCount`, then `monthlyBrokenLimit`, then `tokenId ASC`
- Semantics:
  - no current-month subject record => both `monthlyBroken*` fields are `null`
  - unbound token with records => `monthlyBrokenLimit = 2`

## `GET /api/tokens/:id/broken-keys?page=&per_page=`

- Response shape matches `GET /api/users/:id/broken-keys`.
