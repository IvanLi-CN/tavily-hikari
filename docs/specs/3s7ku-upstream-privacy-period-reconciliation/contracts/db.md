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

## MCP session binding administration

- 异常 `upstream_mcp` session 管理复用代理 session 记录本身，不新增 raw upstream session id 的 owner-facing 投影视图。
- 管理侧查询只返回代理侧 session id、token/user/key 关联、创建/续约/过期时间、派生状态（active/expired/revoked）以及既有 `revoked_at` / `revoke_reason`。
- 首版不新增 `revoked_by`；批量或按筛选释放动作只写入既有 revoke 字段，并且只影响仍然活跃的 `gateway_mode = upstream_mcp` 会话。
