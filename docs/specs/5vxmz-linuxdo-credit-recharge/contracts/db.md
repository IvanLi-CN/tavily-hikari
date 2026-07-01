# DB Contract

## `linuxdo_credit_recharge_orders`

- `out_trade_no TEXT PRIMARY KEY`
- `user_id TEXT NOT NULL`
- `status TEXT NOT NULL`
- `credits INTEGER NOT NULL`
- `months INTEGER NOT NULL`
- `money_cents INTEGER NOT NULL`
- `trade_no TEXT`
- `payment_url TEXT`
- `order_name TEXT NOT NULL`
- `notify_payload TEXT`
- `created_at INTEGER NOT NULL`
- `updated_at INTEGER NOT NULL`
- `paid_at INTEGER`
- `refunded_at INTEGER`
- `refund_actor TEXT`
- `refund_payload TEXT`
- `last_notify_at INTEGER`
- `last_error TEXT`

## `linuxdo_credit_recharge_entitlements`

- `id INTEGER PRIMARY KEY AUTOINCREMENT`
- `out_trade_no TEXT NOT NULL`
- `user_id TEXT NOT NULL`
- `month_start INTEGER NOT NULL`
- `credits INTEGER NOT NULL`
- `created_at INTEGER NOT NULL`
- Unique: `(out_trade_no, month_start)`
- Indexed by `(user_id, month_start)`

This table is retained as a recharge-specific backup ledger. New quota reads use
`account_entitlements`; recharge settlement mirrors rows here so the legacy ledger remains a
rollback anchor.

## `account_entitlements`

- `id INTEGER PRIMARY KEY AUTOINCREMENT`
- `user_id TEXT NOT NULL`
- `scope_kind TEXT NOT NULL`
- `month_start INTEGER NOT NULL`
- `business_calls_1h_delta INTEGER NOT NULL`
- `daily_credits_delta INTEGER NOT NULL`
- `monthly_credits_delta INTEGER NOT NULL`
- `backend_note TEXT NOT NULL`
- `frontend_note TEXT NOT NULL`
- `source_kind TEXT NOT NULL`
- `source_id TEXT NOT NULL`
- `actor_user_id TEXT`
- `actor_display_name TEXT`
- `created_at INTEGER NOT NULL`
- `scope_kind` is `month` or `permanent`.
- `source_kind` is `recharge` for Linux.do Credit payment benefits and `admin` for manual admin adjustments.
- Monthly rows use server-local natural month starts. Permanent rows use `month_start=0`.
- Recharge rows are unique by `(source_id, month_start)` for `source_kind='recharge'`.
- Indexed by `(user_id, scope_kind, month_start)` and `(user_id, created_at)`.

## Semantics

- `month_start` is the UTC timestamp for server-local month start.
- `account_entitlements` is the quota entitlement read source. Effective quota is computed as
  base quota + tag deltas + current-month entitlement deltas + permanent entitlement deltas.
- Entitlements are append-only except when an admin `refund` explicitly revokes a paid recharge
  order's benefits. `refundOnly` keeps entitlement rows.
- Admin-created entitlement rows are never edited or deleted; corrections are represented by
  reverse rows.
- Repeated notifications update order metadata but must not duplicate recharge entitlement rows.
- Existing rows in `linuxdo_credit_recharge_entitlements` are backfilled into `account_entitlements`
  as `source_kind='recharge'` rows while the legacy table remains in place.
- `status` values are `pending`, `paid`, `failed`, `refunding`, `refunded`, and `refundOnly`.
  `refunding` is an internal in-progress reservation used before the external refund call.
- Refund audit details are persisted on the order row; TOTP codes are never stored.

## Admin TOTP meta records

- `admin_totp_secret_ciphertext_v1`: encrypted global TOTP setup secret.
- `admin_totp_secret_nonce_v1`: AEAD nonce for the encrypted secret.
- `admin_totp_enabled_at_v1`: Unix timestamp when the current secret was confirmed.
- `admin_totp_failure_count_v1`: consecutive failed verification count.
- `admin_totp_locked_until_v1`: Unix timestamp for temporary TOTP lockout.

## Admin TOTP semantics

- The TOTP secret uses SHA1, 6 digits, 30-second period, skew `1`.
- The secret is encrypted with `LINUXDO_OAUTH_REFRESH_TOKEN_CRYPT_KEY`.
- Reset and disable require the currently bound TOTP.
