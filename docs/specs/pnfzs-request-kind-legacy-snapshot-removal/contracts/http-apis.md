# HTTP API contracts

## Request / Token log items

- Change type: Delete
- Scope: external

### Removed fields

- `legacyRequestKindKey`
- `legacyRequestKindLabel`
- `legacyRequestKindDetail`

### Preserved behavior

- Canonical fields remain unchanged:
  - `requestKindKey`
  - `requestKindLabel`
  - `requestKindDetail`
- Legacy alias filters remain accepted by the backend and continue to resolve to canonical results before querying.
