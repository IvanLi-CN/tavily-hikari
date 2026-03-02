# Token usage rollup 幂等修复（#k884v）

## 状态

- Status: 已完成
- Created: 2026-03-02
- Last: 2026-03-02

## 背景 / 问题陈述

- `token_usage_stats` 的增量 rollup 使用 `created_at >= last_ts` 边界，遇到同秒日志会重复重扫。
- 写入使用 `ON CONFLICT ... + excluded.*` 累加，重复重扫会导致统计持续漂移。
- 线上核查显示该问题在上个月与最近 24h 都可复现，不是仅历史脏数据导致。

## 目标 / 非目标

### Goals

- 将 rollup 游标改为单调递增的 `auth_token_logs.id`，保证增量幂等。
- 保持现有 HTTP API 与数据结构不变，不中断服务。
- 保持对旧游标 `token_usage_rollup_last_ts` 的一次性兼容迁移。

### Non-goals

- 不做历史 `token_usage_stats` 回填/重建。
- 不调整调度周期（保持 5 分钟）。
- 不修改前端展示口径。

## 范围（Scope）

### In scope

- `src/lib.rs`
  - `rollup_token_usage_stats` 增量逻辑切换到 `id` 游标。
  - 增加 v2 meta key 与 legacy cursor 迁移桥接。
  - 初始化 `auth_token_logs(counts_business_quota, id)` 索引。
  - 新增/调整 rollup 幂等相关测试。
- `docs/specs/README.md`
- `docs/specs/k884v-token-usage-rollup-idempotency/SPEC.md`

### Out of scope

- `src/server.rs` 的调度策略。
- 历史统计纠偏任务。
- 任何外部 API 变更。

## 验收标准（Acceptance Criteria）

- Given 相同数据集无新增日志
  When 连续运行两次 `rollup_token_usage_stats`
  Then 第二次返回 `rows_affected=0` 且统计不增长。

- Given 首次 rollup 后新增一条“同秒但新 id”日志
  When 再次运行 rollup
  Then 仅新增该条的计数，不重复累计旧同秒日志。

- Given 仅存在 legacy 游标 `token_usage_rollup_last_ts`
  When 首次运行新逻辑
  Then 正确生成并写入 `token_usage_rollup_last_log_id_v2`，且不重复累计 legacy 窗口。

## 非功能性验收 / 质量门槛（Quality Gates）

- `cargo test rollup_token_usage_stats_counts_only_billable_logs -- --nocapture`
- `cargo test rollup_token_usage_stats_is_idempotent_without_new_logs -- --nocapture`
- `cargo test rollup_token_usage_stats_processes_same_second_log_once -- --nocapture`
- `cargo test rollup_token_usage_stats_migrates_legacy_timestamp_cursor -- --nocapture`
- `cargo test mcp_tools_list_does_not_increment_billable_totals_after_rollup -- --nocapture`
- `cargo test tavily_http_usage_returns_daily_and_monthly_counts -- --nocapture`
- `cargo test`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 引入 v2 游标 `token_usage_rollup_last_log_id_v2` 与 legacy 桥接
- [x] M2: rollup 增量边界改为 `id > last_log_id AND id <= max_log_id`
- [x] M3: 新增索引 `auth_token_logs(counts_business_quota, id)`
- [x] M4: 新增幂等与迁移回归测试并通过
- [x] M5: 本地全量测试通过并完成本地提交（不 push）

## 实现结果记录

- `src/lib.rs` 已切换 rollup 增量游标到 `auth_token_logs.id`，并保留 legacy 时间游标桥接。
- `token_usage_rollup_last_ts` 仍会同步更新（用于观测与降级兼容），但不再作为增量扫描主游标。
- 新增测试覆盖：
  - `rollup_token_usage_stats_is_idempotent_without_new_logs`
  - `rollup_token_usage_stats_processes_same_second_log_once`
  - `rollup_token_usage_stats_migrates_legacy_timestamp_cursor`

## 后续排查建议（安全、简约）

- 观察窗口统一使用 UTC，优先看最近 24h / 7d；仅统计 `counts_business_quota = 1`。
- 报告中仅展示聚合值与 token 短 ID（例如前 6 位），不输出完整 token、密钥、请求体。
- 连续两个 rollup 周期无新增日志时，`rows_affected` 应为 `0`，且 v2 游标不回退。
- `token_usage_stats` 与 `auth_token_logs` 按 `token_id + hour(bucket_start)` 对账，不应出现持续单向正漂移。
- 若对账差值连续 2 个周期超阈值（建议 `|delta| > 3`），判定异常并进入修复分支。

### 参考 SQL（仅巡检）

```sql
-- Q1: 最近24h，每小时对账（delta = stats - logs）
WITH logs AS (
  SELECT token_id, (created_at / 3600) * 3600 AS bucket_start, COUNT(*) AS c
  FROM auth_token_logs
  WHERE counts_business_quota = 1
    AND created_at >= strftime('%s', 'now') - 86400
  GROUP BY token_id, bucket_start
),
stats AS (
  SELECT token_id, bucket_start,
         (success_count + system_failure_count + external_failure_count + quota_exhausted_count) AS c
  FROM token_usage_stats
  WHERE bucket_secs = 3600
    AND bucket_start >= strftime('%s', 'now') - 86400
),
joined AS (
  SELECT l.token_id,
         l.bucket_start,
         l.c AS logs_c,
         COALESCE(s.c, 0) AS stats_c
  FROM logs l
  LEFT JOIN stats s
    ON s.token_id = l.token_id
   AND s.bucket_start = l.bucket_start
  UNION ALL
  SELECT s.token_id,
         s.bucket_start,
         0 AS logs_c,
         s.c AS stats_c
  FROM stats s
  LEFT JOIN logs l
    ON l.token_id = s.token_id
   AND l.bucket_start = s.bucket_start
  WHERE l.token_id IS NULL
)
SELECT token_id, bucket_start, stats_c - logs_c AS delta
FROM joined
WHERE stats_c <> logs_c
ORDER BY ABS(delta) DESC, bucket_start DESC;

-- Q2: 全局总量漂移（应趋于稳定，不再单向增加）
SELECT
  (SELECT COUNT(*) FROM auth_token_logs WHERE counts_business_quota = 1) AS billable_logs,
  (SELECT COALESCE(SUM(success_count + system_failure_count + external_failure_count + quota_exhausted_count), 0)
   FROM token_usage_stats) AS stats_total;

-- Q3: v2 游标存在性与数值检查
SELECT key, value
FROM meta
WHERE key IN ('token_usage_rollup_last_log_id_v2', 'token_usage_rollup_last_ts');
```
