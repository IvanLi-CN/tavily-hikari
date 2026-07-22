# 上游身份隐私与分段积分对账实现状态（#3s7ku）

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Implementation: 已实现（收口中）
- Lifecycle: validating
- Catalog note: strict upstream headers, compare-vs-precise reconciliation gating, and signed reconciliation.

## Coverage / rollout summary

- 后端已统一三条出站路径的 Header allowlist：Tavily HTTP / Rebalance HTTP 仅保留 `accept`、`accept-encoding`、`content-type` 与策略注入后的 `x-project-id`；Control MCP 仅保留协议恢复头，并按配置选择性注入固定 `user-agent`。
- `SystemSettings` 已持久化 `upstreamProjectIdMode`、`upstreamProjectIdFixedValue`、`upstreamMcpUserAgent`，默认 `accessToken`，并对 fixed/UA 输入做长度与控制字符校验。
- `rebalanceMcpSessionPercent` 与 `apiRebalancePercent` 继续作为兼容字段保留，但运行时与管理端都只按对应开关工作，并统一归一化为 `0|100`。
- `accessToken` 模式已接入 `HMAC-SHA256(secret, "v1" + token_id + period_code)`，业务窗口按服务器本地时区 `S1=00-11`、`S2=11-22`、`S3=22-24` 切分。
- 已落地完整窗口对账、Research 终态等待、24 小时 degraded 兜底、signed reconciliation adjustment 账本，以及对小时/日/月额度的归属修正。
- compare-only 的 `/api/users` / `/admin/users` / `/admin/users/usage` 已统一为 confirmed absolute value or explicit unavailable 语义：相等 delta 不再折叠成空值，未确认 shadow 时明确显示 unavailable。
- shadow compare 与 precise cutover 已拆成两套门禁：即使遗留 `upstream_mcp` session 尚未排空，只要三项静态条件满足，shadow compare 仍会持续产数；precise 仍要求活跃异常 session 清零并等待下一完整窗口。
- 管理端已新增系统设置中的 warning 入口、`/admin/system-settings/status` 系统状态页中的活跃 `upstream_mcp` session 统计卡，以及隐藏路由 `/admin/system-settings/mcp-session-bindings` 的查询/释放管理面。
- 系统状态主相位已纠偏：shadow 已产数但 precise 被旧 session 阻塞时，显示“仅对比”，不再显示“排空旧会话中”。
- reconciliation 运行时已补充 `lastReconciliationRunAt`、`lastShadowAdjustmentAt`、`lastReconciliationEnqueueErrorAt` 三个全局摘要字段，并为 enqueue reuse / exhaustion、run started / completed、shadow adjustment written 输出结构化日志信号。
- reconciliation backlog 诊断已区分 `rate_limited` 的上游 429、本地 usage 限流与其他重试；系统状态页同步展示当前时段每个上游 Key 的绑定用户数与待查询 Project ID 数活动图。
- `upstream_reconciliation` worker 已对同一上游 Key 的到期窗口应用 key-scoped backoff：首次遇到 429 或本地 usage 限流后，本轮复用该 Key 的退避状态，不再反复查询同一 hot key，同时保留其他 Key 的结算机会。

## Remaining Gaps

- 待补最终视觉证据与 owner-facing 截图归档。

## Related Changes

- `src/analysis.rs`
- `src/upstream_privacy.rs`
- `src/tavily_proxy/proxy_http_and_logs.rs`
- `src/tavily_proxy/proxy_quota_sync_and_jobs.rs`
- `src/store/key_store_upstream_reconciliation.rs`
- `src/store/key_store_sessions.rs`
- `src/server/handlers/admin_resources/forward_proxy_and_key_validation.rs`
- `web/src/admin/SystemSettingsModule.tsx`
- `web/src/admin/UpstreamPrivacyStatusModule.tsx`
- `web/src/api/systemSettingsTypes.ts`
- `web/src/styles/clay.css`
- `web/src/admin/McpSessionBindingsModule.tsx`
- `web/src/admin/AdminDashboardRuntime.tsx`

## References

- `./SPEC.md`
- `./HISTORY.md`
- `../../high-anonymity-proxy.md`
