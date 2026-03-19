# DB contracts

## `request_logs`

Add columns:

- `failure_kind TEXT NULL`
- `key_effect_code TEXT NOT NULL DEFAULT 'none'`
- `key_effect_summary TEXT NULL`

## `auth_token_logs`

Add columns:

- `failure_kind TEXT NULL`
- `key_effect_code TEXT NOT NULL DEFAULT 'none'`
- `key_effect_summary TEXT NULL`

## `api_key_maintenance_records`

Append-only audit table:

- `id TEXT PRIMARY KEY`
- `key_id TEXT NOT NULL`
- `source TEXT NOT NULL`
- `operation_code TEXT NOT NULL`
- `operation_summary TEXT NOT NULL`
- `reason_code TEXT NULL`
- `reason_summary TEXT NULL`
- `reason_detail TEXT NULL`
- `request_log_id INTEGER NULL`
- `auth_token_log_id INTEGER NULL`
- `auth_token_id TEXT NULL`
- `actor_user_id TEXT NULL`
- `actor_display_name TEXT NULL`
- `status_before TEXT NULL`
- `status_after TEXT NULL`
- `quarantine_before INTEGER NOT NULL DEFAULT 0`
- `quarantine_after INTEGER NOT NULL DEFAULT 0`
- `created_at INTEGER NOT NULL`

Indexes:

- `(key_id, created_at DESC)`
- `(request_log_id)`
- `(auth_token_log_id)`
