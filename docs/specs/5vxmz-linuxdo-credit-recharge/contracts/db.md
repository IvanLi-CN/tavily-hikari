# DB Contract

## `linuxdo_credit_recharge_orders`

- `out_trade_no TEXT PRIMARY KEY`
- `user_id TEXT NOT NULL`
- `status TEXT NOT NULL`
- `credits INTEGER NOT NULL`
- `months INTEGER NOT NULL`
- `money_cents INTEGER NOT NULL`
- `quote_month_start INTEGER NOT NULL DEFAULT 0`
- `final_money_cents INTEGER NOT NULL DEFAULT 0`
- `final_hourly_delta INTEGER NOT NULL DEFAULT 0`
- `final_daily_delta INTEGER NOT NULL DEFAULT 0`
- `final_monthly_delta INTEGER NOT NULL DEFAULT 0`
- `month_end_clamp_applied INTEGER NOT NULL DEFAULT 0`
- `quote_snapshot_json TEXT`
- `trade_no TEXT`
- `payment_url TEXT`
- `order_name TEXT NOT NULL`
- `notify_payload TEXT`
- `created_at INTEGER NOT NULL`
- `pay_expires_at INTEGER NOT NULL DEFAULT 0`
- `cancel_after_at INTEGER NOT NULL DEFAULT 0`
- `cancelled_at INTEGER`
- `updated_at INTEGER NOT NULL`
- `paid_at INTEGER`
- `refunded_at INTEGER`
- `refund_actor TEXT`
- `refund_payload TEXT`
- `last_notify_at INTEGER`
- `refund_retry_after_at INTEGER`
- `refund_attempts INTEGER NOT NULL DEFAULT 0`
- `last_error TEXT`

## `linuxdo_credit_recharge_entitlements`

- `id INTEGER PRIMARY KEY AUTOINCREMENT`
- `out_trade_no TEXT NOT NULL`
- `user_id TEXT NOT NULL`
- `month_start INTEGER NOT NULL`
- `credits INTEGER NOT NULL`
- `hourly_delta INTEGER NOT NULL DEFAULT 0`
- `daily_delta INTEGER NOT NULL DEFAULT 0`
- `monthly_delta INTEGER NOT NULL DEFAULT 0`
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
- `scope_kind` is `base`, `month`, or `permanent`.
- `source_kind` is `recharge` for Linux.do Credit payment benefits and `admin` for manual admin adjustments.
- Base and permanent rows use `month_start=0`. Monthly rows use server-local natural month starts.
- Recharge rows are unique by `(source_id, month_start)` for `source_kind='recharge'`.
- Indexed by `(user_id, scope_kind, month_start)` and `(user_id, created_at)`.

## Semantics

- `month_start` is the UTC timestamp for server-local month start.
- `account_entitlements` is the quota entitlement read source. Effective quota is computed as
  default account base quota + base entitlement deltas + tag deltas + current-month entitlement
  deltas + permanent entitlement deltas.
- `quotaBase` in admin user detail is displayed as default account base quota plus base entitlement
  deltas. Historical custom `account_quota_limits` rows are migrated into `scope_kind='base'`
  ledger rows and reset to migration-time default storage with `inherits_defaults=0`, so existing
  effective quota remains stable across future default changes.
- Entitlements are append-only except when an admin `refund` explicitly revokes a paid recharge
  order's benefits. `refundOnly` keeps entitlement rows.
- Admin-created entitlement rows are never edited or deleted; corrections are represented by
  reverse rows.
- Repeated notifications update order metadata but must not duplicate recharge entitlement rows.
- Existing rows in `linuxdo_credit_recharge_entitlements` are backfilled into `account_entitlements`
  as `source_kind='recharge'` rows while the legacy table remains in place.
- `status` values are `pending`, `paid`, `failed`, `expired`, `cancelled`, `refunding`,
  `refunded`, and `refundOnly`.
- `expired` means the order passed `pay_expires_at`, so the local UI and APIs must stop exposing
  `payment_url`; a callback that still lands before `cancel_after_at` and within the same
  `quote_month_start` may still settle to `paid`.
- `cancelled` means the order passed `cancel_after_at`; any later success callback must go through
  the automatic refund path instead of granting entitlements.
- `refunding` is used both for admin-initiated refunds and for `refund_actor=system:auto`
  late-payment compensation.
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
