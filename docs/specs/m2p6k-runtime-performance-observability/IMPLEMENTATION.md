# Implementation：性能诊断日志与低内存稳定运行合同（#m2p6k）

## 当前状态

- 状态：实现完成（待最终验证）
- 最近更新：2026-08-02

## 已落地实现

- 默认 runtime logging 继续沿用现有 JSON stderr 与 `RUNTIME_LOG_FORMAT=text` fallback，
  没有引入第二套 telemetry。
- 新增 `RuntimeMemorySnapshot` 与 `RuntimePerfScope`，默认事件可采集：
  - `memory_current_bytes`
  - `memory_limit_bytes`
  - `headroom_bytes`
  - `process_rss_bytes`
  - `child_process_rss_bytes`
  - `process_group_rss_bytes`
  - `process_hwm_bytes`
  - `process_swap_bytes`
- HA 读写链路已补结构化 perf 事件：
  - baseline/events export
  - baseline/events import
  - standby baseline/events sync
  - 三路 channel 的 `outbox_row_count / oldest_age / ack_lag` 只读观测
- HA perf events now use per-`(event, channel)` sampling: ordinary completions are DEBUG with a
  once-per-minute INFO sample, while outbox aggregation plus full runtime-memory capture is
  limited to once per channel every five minutes or occurs immediately for slow work. This keeps
  the diagnostic fields available without adding read pressure on each polling pass. DEBUG samples
  retain source, peer, watermark, cursor, and detail fields so an operator can still trace normal
  synchronization progress without enabling the expensive aggregate collection.
- HA cleanup timing keeps command wall-clock distinct from active cleanup time and the slowest
  cleanup batch. The offline command excludes configured yields and post-cleanup overhead from its
  batch metrics, so its diagnostics remain comparable to the online slice contract.
- SQLx slow-statement emission uses DEBUG instead of WARN. Default runtime logs retain structured
  operation timing/errors without complete statement text; an explicit `sqlx::query=debug` filter
  restores SQL-level diagnostics.
- owner-facing 重读路径已补结构化 perf 事件：
  - dashboard overview
  - dashboard shared snapshot cache-hit / rebuild
  - dashboard phase-level 事件：`freshness_probe` / `cache_wait` / `quota_charge_rebuild` / `recent_alerts_rebuild` / `overview_payload_build` / `overview_serialize`
  - alerts phase-level 事件：`alerts_projection` / `alerts_grouping`
  - global/key request logs catalog / list
  - token request logs catalog / list
- forward-proxy/xray 启动关键阶段已补结构化 perf 事件：
  - runtime begin
  - snapshot persisted
  - store synced
- owner-facing 重读路径新增默认 `low_memory_protection_decision` 事件，用来记录当前判定
  verdict；PR1 阶段先记录既有 `full/cache_hit/rebuilt` 语义，不改变业务响应。
- `low_memory_protection_decision` 已增加 30 秒重复采样抑制，相同 verdict 不再连续刷默认 INFO。
- request logs / token logs 的 perf 完成事件默认走 `INFO` 级别，避免把正常诊断事件误打成
  `WARN`。
- 新增日志单测，直接断言 perf 事件包含稳定字段、phase/outbox 字段与内存预算字段，并确认 `INFO` 级输出可解析。
- `RuntimePerfScope` 延迟到真正输出日志时才读取 footprint；INFO 采样五分钟复用快照，slow/error
  立即采集。cgroup anon/file/swap 与进程 RssAnon/RssFile/VmSwap 分开输出。
- 在线 HA GC、对账和调度恢复现已由结构化状态跃迁日志覆盖；普通切片、phase、逐 key 429 与
  enqueue reuse 不再逐次输出 INFO/WARN。

## 已完成验证

- `cargo fmt`
- `cargo check`
- `cargo test --lib runtime_logging::tests::runtime_memory_helpers_parse_status_and_cgroup_values -- --nocapture`
- `cargo test --lib runtime_logging::tests::runtime_perf_scope_exposes_elapsed_and_memory_fields -- --nocapture`
- `cargo test --lib store::tests::perf_logs_are_info_level_and_include_memory_budget_fields -- --nocapture`
- `cargo test --lib store::tests::low_memory_protection_duplicate_logs_are_sampled -- --nocapture`
- `cargo test --bin tavily-hikari ha_baseline_uses_zstd_and_excludes_call_records -- --nocapture`
- `cargo test dashboard_overview_snapshot_is_reused_within_the_same_freshness_wave -- --nocapture`
- `cargo test admin_logs_cursor_and_catalog_endpoints_expose_retention_without_blocking_page_counts -- --nocapture`
- `cargo test alerts_and_ha -- --nocapture`
- `cargo test log_catalog_and_dashboard_sse -- --nocapture`
- `cargo clippy -- -D warnings`

## 剩余缺口

- 需要在当前最终 SHA 运行全量验证，并补齐管理员 HA/对账 Storybook 的新视觉证据。
- HA outbox 观测现已覆盖在线 self-healing 的累计与最慢 cleanup micro-batch SQL 耗时、续片延迟、累计删除、高水位增量与
  ingress-minus-delete 估算；后置状态 probe 不参与批次耗时。它们不能替代精确库存统计，但可低成本确认过期债务是否持续前移。
- Deferred-continuation diagnostics share one durable transaction with the selected channel's
  pending-debt bit. A cleared global mask therefore cannot suppress the watchdog signal for a
  failed continuation write, while normal clean-state polling remains quiet.
- HA export/sync 的低频样本使用 `outbox_sequence_span_estimate` 和 `outbox_high_watermark`，不再对每个通道
  执行 `COUNT(*)` 或把近似 span 标成精确 `outbox_row_count`。

## 相关文件

- `src/runtime_logging.rs`
- `src/store/mod.rs`
- `src/store/key_store_ha.rs`
- `src/store/key_store_request_logs_and_dashboard.rs`
- `src/store/key_store_token_logs.rs`
- `src/server/handlers/admin_resources/ha.rs`
- `src/server/handlers/public.rs`
- `src/server/serve.rs`
- `src/tavily_proxy/proxy_core.rs`
- HA perf events retain stable structured fields while avoiding false ACK lag: outbox stats only
  compute lag when a peer watermark is supplied, and the admin health path uses watermark plus
  indexed `EXISTS` checks instead of row counts.

## Reconciliation pressure telemetry

- Reconciliation persists a global pressure streak and backoff level. Three consecutive runs with
  zero settlements and predominantly upstream 429 responses enter a 2/5/10/30 minute backoff;
  per-key cooldown remains authoritative, while normal per-key backoff logs are DEBUG and the
  state transition is summarized once at WARN. The administrative system-status surface exposes the
  current pressure streak, level, and retry time.

## Low-cost memory and transition telemetry

- INFO memory collection is cached for five minutes; slow and error paths bypass the cache.
- cgroup anon/file/swap and process RssAnon/RssFile/VmSwap are reported separately.
- Normal HA slices, dashboard phases, and per-key rate limits are DEBUG; actionable states emit only
  enter, escalation, and recovery transitions.

## Visual Evidence

PR: include

![System status global reconciliation backoff](./assets/system-status-global-reconciliation-backoff.png)

- Storybook canvas: `Admin/Modules/SystemStatusModule/GlobalBackoff`
- evidence_note: Mock-only system-status state showing the persisted global reconciliation backoff
  after three pressure runs. Bound to `525c07d305537a6c60cf176d28f8ec9788895424`; it must be
  recaptured after any UI or fixture change.
