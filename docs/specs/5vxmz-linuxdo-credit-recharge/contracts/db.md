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

## Semantics

- `month_start` is the UTC timestamp for server-local month start.
- Entitlements are append-only after successful payment except when an admin `refund` explicitly
  revokes the order benefits. `refundOnly` keeps entitlements.
- Repeated notifications update order metadata but must not duplicate entitlement rows.
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
