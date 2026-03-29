# Database contracts

## request_logs

- Change type: Delete
- Scope: internal

### Removed columns

- `legacy_request_kind_key`
- `legacy_request_kind_label`
- `legacy_request_kind_detail`

## auth_token_logs

- Change type: Delete
- Scope: internal

### Removed columns

- `legacy_request_kind_key`
- `legacy_request_kind_label`
- `legacy_request_kind_detail`

## Migration requirements

- Startup migration must rebuild both tables when these columns are still present.
- Repeated startup on an already-upgraded database must be a no-op.
