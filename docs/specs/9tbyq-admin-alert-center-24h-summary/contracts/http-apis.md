# HTTP APIs

## `GET /api/alerts/catalog`

### Query

- none

### Response

```json
{
  "retentionDays": 30,
  "types": [
    { "value": "upstream_rate_limited_429", "count": 12 },
    { "value": "upstream_usage_limit_432", "count": 7 },
    { "value": "upstream_key_blocked", "count": 2 },
    { "value": "user_request_rate_limited", "count": 9 },
    { "value": "user_quota_exhausted", "count": 4 }
  ],
  "requestKindOptions": [
    { "key": "search", "label": "Search", "count": 8, "protocolGroup": "api", "billingGroup": "billable" }
  ],
  "users": [
    { "value": "usr_alice", "label": "Alice Wang", "count": 5 }
  ],
  "tokens": [
    { "value": "qa13", "label": "qa13", "count": 7 }
  ],
  "keys": [
    { "value": "key_live_001", "label": "key_live_001", "count": 6 }
  ]
}
```

## `GET /api/alerts/events`

### Query

- `page?: number`
- `per_page?: number`
- `type?: string`
- `since?: iso8601`
- `until?: iso8601`
- `user_id?: string`
- `token_id?: string`
- `key_id?: string`
- `request_kind?: string` (repeatable)

### Response

```json
{
  "items": [
    {
      "id": "atl:1823",
      "type": "upstream_usage_limit_432",
      "title": "Alice Wang hit Tavily usage limit",
      "summary": "Token qa13 received Tavily usage-limit 432 for Search via key_live_001.",
      "occurredAt": 1765389200,
      "subjectKind": "user",
      "subjectId": "usr_alice",
      "subjectLabel": "Alice Wang",
      "user": { "userId": "usr_alice", "displayName": "Alice Wang", "username": "alice" },
      "token": { "id": "qa13", "label": "qa13" },
      "key": { "id": "key_live_001", "label": "key_live_001" },
      "request": { "id": 8841, "method": "POST", "path": "/api/tavily/search" },
      "requestKind": { "key": "search", "label": "Search", "detail": null },
      "failureKind": null,
      "resultStatus": "quota_exhausted",
      "source": { "kind": "auth_token_log", "id": 1823 }
    }
  ],
  "total": 1,
  "page": 1,
  "perPage": 20
}
```

## `GET /api/alerts/groups`

### Query

- 与 `GET /api/alerts/events` 完全一致。

### Response

```json
{
  "items": [
    {
      "id": "upstream_usage_limit_432:user:usr_alice:search",
      "type": "upstream_usage_limit_432",
      "subjectKind": "user",
      "subjectId": "usr_alice",
      "subjectLabel": "Alice Wang",
      "requestKind": { "key": "search", "label": "Search", "detail": null },
      "key": { "id": "key_live_001", "label": "key_live_001" },
      "count": 4,
      "firstSeen": 1765388200,
      "lastSeen": 1765389200,
      "latestEvent": {
        "id": "atl:1823",
        "title": "Alice Wang hit Tavily usage limit",
        "summary": "Token qa13 received Tavily usage-limit 432 for Search via key_live_001."
      }
    }
  ],
  "total": 1,
  "page": 1,
  "perPage": 20
}
```
