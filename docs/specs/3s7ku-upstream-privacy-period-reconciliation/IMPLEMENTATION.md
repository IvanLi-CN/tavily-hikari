# 上游身份隐私与分段积分对账实现状态（#3s7ku）

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Implementation: 已实现
- Lifecycle: merge-ready
- Catalog note: strict upstream headers, period-scoped pseudonymous identity, and signed reconciliation.

## Coverage / rollout summary

- 后端已统一三条出站路径的 Header allowlist：Tavily HTTP / Rebalance HTTP 仅保留 `accept`、`accept-encoding`、`content-type` 与策略注入后的 `x-project-id`；Control MCP 仅保留协议恢复头，并按配置选择性注入固定 `user-agent`。
- `SystemSettings` 已持久化 `upstreamProjectIdMode`、`upstreamProjectIdFixedValue`、`upstreamMcpUserAgent`，默认 `accessToken`，并对 fixed/UA 输入做长度与控制字符校验。
- `rebalanceMcpSessionPercent` 与 `apiRebalancePercent` 继续作为兼容字段保留，但运行时与管理端都只按对应开关工作，并统一归一化为 `0|100`。
- `accessToken` 模式已接入 `HMAC-SHA256(secret, "v1" + token_id + period_code)`，业务窗口按服务器本地时区 `S1=00-11`、`S2=11-22`、`S3=22-24` 切分。
- 已落地完整窗口对账、Research 终态等待、24 小时 degraded 兜底、signed reconciliation adjustment 账本，以及对小时/日/月额度的归属修正。
- 管理端已新增系统设置中的上游身份配置控件、`/admin/system-settings/status` 系统状态页，并移除了独立的 rebalance 比例控件；对应 Storybook coverage 与 mock-only UI 证据已更新为开关语义。

## Remaining Gaps

- 无已知功能缺口；剩余仅为 PR 创建、review convergence 与合并路径动作。

## Related Changes

- `src/analysis.rs`
- `src/upstream_privacy.rs`
- `src/tavily_proxy/proxy_http_and_logs.rs`
- `src/tavily_proxy/proxy_quota_sync_and_jobs.rs`
- `src/store/key_store_upstream_reconciliation.rs`
- `web/src/admin/SystemSettingsModule.tsx`
- `web/src/admin/UpstreamPrivacyStatusModule.tsx`
- `web/src/admin/AdminDashboardRuntime.tsx`

## References

- `./SPEC.md`
- `./HISTORY.md`
- `../../high-anonymity-proxy.md`
