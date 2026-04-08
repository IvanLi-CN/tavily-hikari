# DB contract

## `oauth_accounts`

Add columns:

- `refresh_token_ciphertext TEXT NULL`
- `refresh_token_nonce TEXT NULL`
- `last_profile_sync_attempt_at INTEGER NULL`
- `last_profile_sync_success_at INTEGER NULL`
- `last_profile_sync_error TEXT NULL`

Semantics:

- `refresh_token_ciphertext` + `refresh_token_nonce` store the encrypted LinuxDO refresh token.
- When a refresh succeeds and returns a new non-empty refresh token, these fields are rotated in-place.
- When refresh succeeds without a replacement refresh token, existing ciphertext/nonce remain unchanged.
- `last_profile_sync_attempt_at` updates on every scheduled sync attempt.
- `last_profile_sync_success_at` updates only on successful profile refresh.
- `last_profile_sync_error` stores a short diagnostic summary for the latest failed scheduled sync; cleared on success.

## `scheduled_jobs`

No shape change.

New `job_type` value:

- `linuxdo_user_status_sync`

Message format should summarize:

- attempted account count
- success count
- skipped count
- failure count
- first failure summary when failures exist
