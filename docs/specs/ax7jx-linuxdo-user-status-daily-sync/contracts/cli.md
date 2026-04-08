# CLI / ENV contract

## New runtime config

- `--linuxdo-oauth-refresh-token-crypt-key` / `LINUXDO_OAUTH_REFRESH_TOKEN_CRYPT_KEY`
  - Required only when `LINUXDO_OAUTH_USER_SYNC_ENABLED=true` and LinuxDO user sync should actually persist/use refresh tokens.
  - Accepts either:
    - a raw UTF-8 string whose byte length is exactly `32`, or
    - a base64 / base64url encoded value that decodes to exactly `32` bytes.
- `--linuxdo-oauth-user-sync-enabled` / `LINUXDO_OAUTH_USER_SYNC_ENABLED`
  - Boolean, default `true`.
  - When `false`, login may still persist refresh token if crypt key is configured, but the daily scheduler must not run.
- `--linuxdo-oauth-user-sync-at` / `LINUXDO_OAUTH_USER_SYNC_AT`
  - `HH:mm`, server local timezone, default `06:20`.

## Validation

- Invalid sync time falls back to `06:20`.
- Invalid crypt key disables refresh-token persistence and daily LinuxDO user sync; server startup remains successful but logs a clear warning.
