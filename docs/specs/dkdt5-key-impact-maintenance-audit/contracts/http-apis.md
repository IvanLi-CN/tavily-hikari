# HTTP API contracts

## Admin logs

### `GET /api/logs`

Each log item adds:

- `failureKind: string | null`
- `keyEffectCode: string`
- `keyEffectSummary: string | null`

All existing admin-only fields remain unchanged.

### `GET /api/tokens/:id/logs/page`

Each log item adds:

- `failureKind: string | null`
- `keyEffectCode: string`
- `keyEffectSummary: string | null`

All existing admin token log fields remain unchanged.

## User/public logs

### `GET /api/user/tokens/:id/logs`

### `GET /api/public/token_logs`

- No new fields.
- Existing `errorMessage` may contain a redacted guidance sentence for error classes `1-5`.
- Existing payload shape and key names remain unchanged.

## Admin health maintenance actions

### `DELETE /api/keys/:id/quarantine`

- Response shape unchanged.
- On successful clear, server appends one maintenance audit row with `operation_code=manual_clear_quarantine`.

### Existing admin mark-exhausted path

- Response shape unchanged.
- On successful mark, server appends one maintenance audit row with `operation_code=manual_mark_exhausted`.
