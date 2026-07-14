# Database and HA contracts

## Secret

- `meta.upstream_project_id_hmac_secret_v1`: 32-byte random secret encoded as Base64URL-no-pad。
- 属于 HA control-plane 同步资源；不得出现在普通 settings/status API 或日志。

## Reconciliation usage

- 记录实际使用组合的 token id、upstream key id、period code、匿名 project id、local billed credits、
  Research pending count、first/last used time 与 eligibility epoch。
- 唯一键覆盖 `(token_id, upstream_key_id, period_code)`。

## Settlement and adjustment

- settlement 唯一键覆盖 `(version, token_id, period_code)`，状态包含 pending/waiting/rate_limited/settled/degraded/skipped。
- signed adjustment 独立保存 `delta_credits`、billing subject、原窗口归属时间、原因与 audit 时间。
- adjustment 行通过 settlement 唯一键保持幂等，并进入 HA billing channel。
