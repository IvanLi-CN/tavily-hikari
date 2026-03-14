# DB

## `forward_proxy_settings`

- singleton row (`id = 1`)
- columns:
  - `proxy_urls_json TEXT NOT NULL DEFAULT '[]'`
  - `subscription_urls_json TEXT NOT NULL DEFAULT '[]'`
  - `subscription_update_interval_secs INTEGER NOT NULL DEFAULT 3600`
  - `insert_direct INTEGER NOT NULL DEFAULT 1`
  - `updated_at INTEGER NOT NULL`

## `forward_proxy_runtime`

- key: `proxy_key TEXT PRIMARY KEY`
- columns:
  - `display_name TEXT NOT NULL`
  - `source TEXT NOT NULL`
  - `endpoint_url TEXT`
  - `weight REAL NOT NULL`
  - `success_ema REAL NOT NULL`
  - `latency_ema_ms REAL`
  - `consecutive_failures INTEGER NOT NULL DEFAULT 0`
  - `is_penalized INTEGER NOT NULL DEFAULT 0`
  - `updated_at INTEGER NOT NULL`

## `forward_proxy_attempts`

- key: autoincrement `id`
- columns:
  - `proxy_key TEXT NOT NULL`
  - `is_success INTEGER NOT NULL`
  - `latency_ms REAL`
  - `failure_kind TEXT`
  - `is_probe INTEGER NOT NULL DEFAULT 0`
  - `occurred_at INTEGER NOT NULL`

## `forward_proxy_weight_hourly`

- key: `(proxy_key, bucket_start_epoch)`
- columns:
  - `sample_count INTEGER NOT NULL`
  - `min_weight REAL NOT NULL`
  - `max_weight REAL NOT NULL`
  - `avg_weight REAL NOT NULL`
  - `last_weight REAL NOT NULL`
  - `last_sample_epoch_us INTEGER NOT NULL`
  - `updated_at INTEGER NOT NULL`

## `forward_proxy_key_affinity`

- key: `key_id TEXT PRIMARY KEY`
- columns:
  - `primary_proxy_key TEXT`
  - `secondary_proxy_key TEXT`
  - `updated_at INTEGER NOT NULL`

## Indices

- `forward_proxy_attempts(proxy_key, occurred_at)`
- `forward_proxy_attempts(occurred_at, proxy_key)`
- `forward_proxy_weight_hourly(bucket_start_epoch, proxy_key)`
