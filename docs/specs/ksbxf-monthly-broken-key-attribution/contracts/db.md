# Database

## `account_quota_limits`

- Added column:
  - `monthly_broken_limit INTEGER NOT NULL DEFAULT 5`

## `token_api_key_bindings`

- Purpose:
  - persist recent successful token ↔ key bindings
  - limit to the latest 3 keys per token by `last_success_at DESC`

Columns:

- `token_id TEXT NOT NULL`
- `api_key_id TEXT NOT NULL`
- `created_at INTEGER NOT NULL`
- `updated_at INTEGER NOT NULL`
- `last_success_at INTEGER NOT NULL`

Keys and indexes:

- `PRIMARY KEY (token_id, api_key_id)`
- recent-by-token index for prune/query
- reverse lookup index by `api_key_id`

## `subject_key_breakages`

- Purpose:
  - persist monthly unique broken-key attribution for `user` and `token` subjects

Columns:

- `subject_kind TEXT NOT NULL`
- `subject_id TEXT NOT NULL`
- `api_key_id TEXT NOT NULL`
- `month_start INTEGER NOT NULL`
- `latest_break_at INTEGER NOT NULL`
- `key_status TEXT NOT NULL`
- `reason_code TEXT NULL`
- `reason_summary TEXT NULL`
- `source TEXT NOT NULL`
- `breaker_token_id TEXT NULL`
- `breaker_user_id TEXT NULL`
- `breaker_user_display_name TEXT NULL`
- `manual_actor_display_name TEXT NULL`
- `created_at INTEGER NOT NULL`
- `updated_at INTEGER NOT NULL`

Keys and indexes:

- `PRIMARY KEY (subject_kind, subject_id, api_key_id, month_start)`
- subject+month index for counts/detail pages
- key+month index for manual fan-out and backfill
