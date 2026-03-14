# HTTP APIs

## `GET /api/settings`

- Auth: admin only
- Response adds `forwardProxy`:

```json
{
  "forwardProxy": {
    "proxyUrls": ["http://127.0.0.1:8080"],
    "subscriptionUrls": ["https://example.com/subscription.base64"],
    "subscriptionUpdateIntervalSecs": 3600,
    "insertDirect": true,
    "nodes": [
      {
        "key": "http://127.0.0.1:8080",
        "source": "manual",
        "displayName": "127.0.0.1:8080",
        "endpointUrl": "http://127.0.0.1:8080",
        "weight": 0.8,
        "penalized": false,
        "primaryAssignmentCount": 2,
        "secondaryAssignmentCount": 1,
        "stats": {
          "oneMinute": { "attempts": 0 },
          "fifteenMinutes": { "attempts": 0 },
          "oneHour": { "attempts": 0 },
          "oneDay": { "attempts": 0 },
          "sevenDays": { "attempts": 0 }
        }
      }
    ]
  }
}
```

## `PUT /api/settings/forward-proxy`

- Auth: admin only
- Request:

```json
{
  "proxyUrls": ["http://127.0.0.1:8080"],
  "subscriptionUrls": ["https://example.com/subscription.base64"],
  "subscriptionUpdateIntervalSecs": 3600,
  "insertDirect": true
}
```

- Response: same shape as `forwardProxy` from `GET /api/settings`
- Semantics:
  - normalize and persist settings
  - refresh subscription if configured
  - sync Xray routes for share-link nodes
  - keep previous runtime/affinity when nodes still exist

## `POST /api/settings/forward-proxy/validate`

- Auth: admin only
- Request:

```json
{
  "kind": "proxyUrl",
  "value": "vmess://..."
}
```

- `kind`: `proxyUrl | subscriptionUrl`
- Response:

```json
{
  "ok": true,
  "message": "subscription validation succeeded",
  "normalizedValue": "https://example.com/subscription.base64",
  "discoveredNodes": 3,
  "latencyMs": 182.5
}
```

- Semantics:
  - single proxy candidate succeeds only if parse + probe succeeds
  - subscription candidate succeeds only if at least one parsed node passes probe
  - parse / Xray / timeout / transport failures return `ok=false` with stable message

## `GET /api/stats/forward-proxy`

- Auth: admin only
- Response:

```json
{
  "rangeStart": "2026-03-12T00:00:00Z",
  "rangeEnd": "2026-03-13T00:00:00Z",
  "bucketSeconds": 3600,
  "nodes": [
    {
      "key": "direct",
      "source": "direct",
      "displayName": "Direct",
      "endpointUrl": null,
      "weight": 1.0,
      "penalized": false,
      "primaryAssignmentCount": 0,
      "secondaryAssignmentCount": 2,
      "stats": {
        "oneMinute": { "attempts": 0 },
        "fifteenMinutes": { "attempts": 0 },
        "oneHour": { "attempts": 0 },
        "oneDay": { "attempts": 0 },
        "sevenDays": { "attempts": 0 }
      },
      "last24h": [
        {
          "bucketStart": "2026-03-12T00:00:00Z",
          "bucketEnd": "2026-03-12T01:00:00Z",
          "successCount": 0,
          "failureCount": 0
        }
      ],
      "weight24h": [
        {
          "bucketStart": "2026-03-12T00:00:00Z",
          "bucketEnd": "2026-03-12T01:00:00Z",
          "sampleCount": 1,
          "minWeight": 1.0,
          "maxWeight": 1.0,
          "avgWeight": 1.0,
          "lastWeight": 1.0
        }
      ]
    }
  ]
}
```
