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
- compare-only 的 `/api/users` / `/admin/users` / `/admin/users/usage` 已统一为 hybrid shadow 语义：始终返回 `当前本地 24h + 已确认 shadow delta`；相等 delta 不再折叠成空值，未完全对账时明确标记为 `projected` 并提示“含未对账估算”。
- `ReconciliationController` 将既有 precise-reconciliation 开关映射为可复制的 `compare`、
  `active` 或 `active_paused` 状态。写 true 立即记录下一完整业务窗口为 billing boundary；边界前
  继续 shadow，历史 observed 不重放。integrity pause 清除旧开关并要求一次新的 true 写入恢复。
- 管理端已新增系统设置中的 warning 入口、`/admin/system-settings/status` 系统状态页中的活跃 `upstream_mcp` session 统计卡，以及隐藏路由 `/admin/system-settings/mcp-session-bindings` 的查询/释放管理面。
- 系统状态主相位已纠偏：shadow 已产数但 precise 被旧 session 阻塞时，显示“仅对比”，不再显示“排空旧会话中”。
- reconciliation 运行时已补充 `lastReconciliationRunAt`、`lastShadowAdjustmentAt`、`lastReconciliationEnqueueErrorAt` 三个全局摘要字段，并为 enqueue reuse / exhaustion、run started / completed、shadow adjustment written 输出结构化日志信号。
- reconciliation backlog 诊断已区分 `rate_limited` 的上游 429、本地 usage 限流与其他重试；系统状态页同步展示当前时段每个上游 Key 的绑定用户数与待查询 Project ID 数活动图。
- `upstream_reconciliation` worker 已对同一上游 Key 的到期窗口应用 key-scoped backoff：首次遇到 429 或本地 usage 限流后，本轮复用该 Key 的退避状态，不再反复查询同一 hot key，同时保留其他 Key 的结算机会。
- reconciliation 候选调度已切成 recent/backlog 双车道：今日+昨日窗口优先按 `period_end DESC` 取最多 `12` 条，旧 backlog 再按 `period_end ASC` 取最多 `8` 条，空余预算双向回填，避免近期窗口长期被旧积压饿死。
- Research 记录已加入持久化 poll 元数据；每轮现有 reconciliation job 先执行最多 20 条、每 Key 最多 4 条的 terminal sweep，历史 pending 自动进入恢复队列。终态写入与 settlement 均保持幂等。
- 对账限流已改用 `period_reconciliation` 独立 Key cooldown：429 不再扇出写入同 Key 的全部窗口，其他 Key 可继续结算；状态 API/页面提供今日账号与账期覆盖、Research 收敛和 per-Key cooldown 进度。
- `/api/users` compare-only 项新增 observed/standard-settled/degraded period count，用户列表和用量页在 hybrid 值旁展示标准对账覆盖及降级数。
- reconciliation keeps a one-at-a-time remote attempt lease only while an outbound HTTP request is
  active and remains under the 20-second total budget. Local projection, hydrate, finalization, and
  Research bookkeeping do not hold that lease; a 120-second eligible automatic representative owns
  the next non-manual request turn. Global 429 pressure state and the delayed representative remain
  durable, avoiding minute-by-minute empty work.
- 候选页先以 period/settlement 索引限制扫描，再做有界 hydrate 与 Research `EXISTS` 判断；执行
  前后不再运行精确队列聚合。系统状态读取 bounded observation，未首次观测时保留 unknown/null
  语义。
- 本地压力连续三轮触发 `30/60/120/300` 秒退避；真实 upstream 429 独立触发
  `2/5/10/30` 分钟退避并尊重更晚的 `Retry-After`。transport、semantic failure 与本地预算耗尽
  不会清空已有 429 状态。
- `upstream_reconciliation_work` 由 usage 写入增量维护。升级前历史行由
  `ReconciliationProjectionController` 使用稳定复合 keyset 和 `25..100` 行自适应微片吸收：读扫描
  在事务外完成，work merge、claim fence、cursor CAS 和进度在同一短事务提交。低压续跑最快一秒，
  busy、慢片或前台压力只产生持久化 typed defer；候选查询不再每轮聚合原始 usage 全表。
- `ReconciliationEngine` uses typed terminal outcomes. A successful upstream observation that
  requires no signed adjustment completes only the matching usage generation as `no_adjustment`;
  compare-mode non-zero deltas complete as `observed` without writing billing truth, while active
  non-zero deltas alone complete as `settled`. Transport, semantic, upstream-429, and local-pressure
  retry state remain independent. The status projection exposes phase timing and outcome counts.
- Claimed reconciliation reserves two seconds for finalization. A reserve exhaustion, admission
  defer, or transient SQLite pressure before a durable boundary exposes a typed deferred outcome;
  stale claims are ignored; other non-transient failures remain terminal scheduler errors rather
  than being silently downgraded. A deferred finalization uses one
  `ScheduledJobControl` transaction for the claim fence, local-backoff state, local-pressure
  observation, retry time, and single auto representative; failure to acquire that transaction
  intentionally retains the running claim for stale recovery. Once the foreground gate reports low
  pressure, its still-current claim clears the local-pressure backoff before entering the engine;
  a fresh SQLite admission failure immediately records a new typed defer instead of suppressing
  the recovery tail with an empty completion.
- Research starvation is evaluated independently of settlement retry buckets. The operational
  acceptance window is ten minutes: terminal rate must become positive while pending Research does
  not grow. `upstream429` remains a rate-limited settlement bucket and is not evidence of Research
  convergence or non-convergence.

## Remaining Gaps

- 101 双库快照对比仍受共享 testbox 可用空间限制；本地与 Storybook 门禁已完成。

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

## Current performance contract

- Main settlement now hydrates the bounded candidate page and all referenced key/cooldown state before starting the first `/usage` request; the research terminal sweep runs only after main settlement and may use at most two seconds of the remaining job time. Its timeout is not primary budget exhaustion.
- Local budget exhaustion is persisted separately from the upstream-429 backoff. Only observed upstream 429 attempts advance the 2/5/10/30-minute global state; local pressure uses a short independent delay and never fabricates a remote rate-limit signal.
- Remote request start, observation, settlement finalization, and research bookkeeping use nested
  deadlines with reserved post-processing headroom. The four local-pressure meta keys are included
  in HA incremental and baseline replication so failover retains the same recovery state.
- Transport and semantic failures preserve an active upstream-429 circuit. A real remote attempt
  clears local-pressure state; only `settled` or `no_adjustment` recovery clears the upstream
  circuit. This prevents unrelated local outcomes from restarting the remote-rate-limit loop.
- `upstream_reconciliation_run_observation.last_transport_kind` is an additive local diagnostic
  column. Versioned observation state also records `last_transport_kind_at` and the latest
  retryable outcome. A later non-transport run preserves the last transport category and timestamp;
  only a terminal result clears the retryable outcome. The migration adds columns without scanning
  work, emitting HA outbox events, or changing billing truth.
