# Runtime：性能诊断日志与低内存稳定运行合同（#m2p6k）

## 背景

- 101 线上实例已经观察到进程组内存升到约 `438MiB`，其中主进程匿名 RSS 约 `375MiB`，历史
  `VmHWM`/`VmSwap` 远高于当前常驻值，说明需要一套能直接从默认 runtime logs 里定位大对象链路的
  稳定证据面。
- 当前已确认的热点跨越 HA baseline/events export/import、standby sync、dashboard shared
  snapshot，以及 owner-facing request logs list/catalog 读路径。
- 现有日志基线已经是默认 JSON stderr、`RUNTIME_LOG_FORMAT=text` fallback、`RUST_LOG`
  过滤、慢 SQL `250ms` 与 DB phase `1s`。新的性能诊断必须扩这个合同，而不是再加一套独立
  telemetry。

## Goals

- 默认 runtime logs 直接暴露关键性能链路的稳定结构化事件，无需临时改代码或打开 debug dump。
- 所有与 `256MiB` 稳定运行合同相关的关键路径都能在日志中看到内存头寸、作用域、耗时和结果。
- 业务调用缓存只在最近一小时保留逐事件数据，前 1–25 小时使用五分钟桶聚合；backfill 使用
  500 行分页和 generation/tail 合并。分页期间必须保留 last-good 快照，只有完整构建成功后才原子交换；
  混合负载下以进程组 RSS P95 `<=256MiB` 作为 SLO 观测目标。
- `/proc` 与 cgroup footprint 只在五分钟采样窗口、慢请求、错误或状态跃迁时读取；不设置
  `memory.max`，也不将 file cache/swap 误判为堆泄漏。

## Non-goals

- 不新增 Prometheus/OTel/owner-facing metrics 页面。
- 不改变 HA、dashboard、request logs、forward-proxy startup 的公开业务 contract。
- 不把秘密、header/body 明文或全量 SQL debug 输出进默认日志。

## Runtime Logging Contract

- 普通 Dashboard phase、HA GC slice、逐 key 429 与 enqueue reuse 使用 DEBUG；聚合运行摘要最多每
  60 秒输出一次 INFO。
- HA GC slice 的 DEBUG 事件还必须区分 wall-clock `elapsed_ms` 与 active database work
  `active_elapsed_ms`、`max_batch_elapsed_ms`，并记录 `deleted_rows` 与 `continuation_delay_secs`，使短让步与 SQLite
  writer contention 不会被混为同一种性能问题。active 字段只包含 cleanup micro-batch，不包含 post-slice
  state probe。
- HA export/sync 的窗口采样以 `outbox_sequence_span_estimate` 与 `outbox_high_watermark` 表示积压趋势；不得在
  该热路径计算或记录名为 `outbox_row_count` 的精确库存。
- 内存 INFO 快照最多每 5 分钟真实读取一次 `/proc` 与 cgroup；slow/error 事件立即采集。
- SQLite workload 以固定 operation/workload class 在有界内存窗口聚合；每 60 秒最多一条
  `component=db event=sqlite_workload_window` INFO，报告调用量、pool/begin wait、transaction hold、
  retries、logical rows、admitted/deferred 与原因、当前/峰值 pool acquire waiter、窗口最小 idle、
  错误/丢弃连接以及 process/cgroup write-byte delta。不得记录 SQL、参数或请求正文。
- `ha_outbox_gc_watchdog` 是短 `maintenance_control` state read，按同一窗口归因；它不输出逐次
  INFO/WARN，也不收集或输出 outbox inventory。
- 内存字段同时报告 cgroup `anon/file/swap` 与进程 `RssAnon/RssFile/VmSwap`，避免把文件页缓存
  误诊为堆泄漏。
- 事务污染、stale claim recovery、连续零进展、预算耗尽和全局退避只在进入、升级或恢复时告警。
- HA GC 低压恢复、SLO deadline、最老可删事件年龄与真实删除率必须可从管理员状态和聚合日志
  还原；sequence span 仅作趋势估算，不作为库存或 ETA。
- Request-log GC admission and an unsealed-day retention guard are DEBUG-level typed outcomes. They
  retain the existing five-minute continuation but must not emit one WARN or repeat one failed
  cleanup batch per scheduler loop.
- 对账主结算必须先于 Research sweep；本地预算压力与 upstream 429 必须分开记录，且最终
  远端观察、结算和状态落盘都必须受同一轮预算约束。HA GC 正常进展继续按通道 60 秒聚合，
  不能恢复逐片 WARN。

- 继续使用默认 `RUNTIME_LOG_FORMAT=json` + `stderr` 输出，保留 `text` fallback。
- 新增的性能事件必须使用现有 `tracing` 结构化字段，按事件适用性包含：
  - `component`
  - `event`
  - `elapsed_ms`
  - 作用域字段：`route` / `scope` / `phase` / `channel` / `page_size` / `row_count` / `degraded`
  - 预算字段：`memory_current_bytes` / `memory_limit_bytes` / `headroom_bytes`
- 若可得，补充：
  - `process_rss_bytes`
  - `child_process_rss_bytes`
  - `process_group_rss_bytes`
  - `process_hwm_bytes`
  - `process_swap_bytes`
  - `payload_bytes`
  - `compressed_bytes`
  - `high_watermark`
  - `outbox_sequence_span_estimate`（仅趋势估算，不是库存）
  - `outbox_oldest_age_secs`
  - `outbox_ack_lag`
- HA normal completion events may be emitted at DEBUG, with an INFO sample per `(event, channel)`
  at most once per minute. Outbox aggregation and full runtime-memory snapshots may be collected at
  most once per channel every five minutes, except for slow operations.
- Default SQLx logging must not emit complete statements at WARN. Stable operation-level logs retain
  timing, rows, and error category; `RUST_LOG=sqlx::query=debug` remains the explicit opt-in for
  raw SQL diagnostics.

## Required Perf Events

- HA:
  - `component=ha event=baseline_export_completed`
  - `component=ha event=events_export_completed`
  - `component=ha event=baseline_import_completed`
  - `component=ha event=events_import_completed`
  - `component=ha event=standby_sync_baseline_completed`
  - `component=ha event=standby_sync_events_completed`
- Dashboard / shared snapshot:
  - `component=startup event=dashboard_overview_prewarmed`
  - `component=startup event=dashboard_overview_prewarm_deferred`
  - `component=admin_read event=dashboard_snapshot_cache_hit`
  - `component=admin_read event=dashboard_snapshot_rebuilt`
  - `component=admin_read event=dashboard_overview_phase phase=freshness_probe`
  - `component=admin_read event=dashboard_overview_phase phase=cache_wait`
  - `component=admin_read event=dashboard_overview_phase phase=quota_charge_rebuild`
  - `component=admin_read event=dashboard_overview_phase phase=recent_alerts_rebuild`
  - `component=admin_read event=dashboard_overview_phase phase=overview_payload_build`
  - `component=admin_read event=dashboard_overview_phase phase=overview_serialize`
- Owner-facing recent request reads:
  - `component=admin_read event=request_logs_catalog_completed`
  - `component=admin_read event=request_logs_list_completed`
  - `component=admin_read event=token_logs_catalog_completed`
  - `component=admin_read event=token_logs_list_completed`
  - `component=admin_read event=/api/alerts/events phase=alerts_projection`
  - `component=admin_read event=/api/alerts/groups phase=alerts_grouping`
  - `component=admin_read event=low_memory_protection_decision`
- Forward proxy / xray startup:
  - `component=forward_proxy event=startup_runtime_begin`
  - `component=forward_proxy event=startup_runtime_snapshot_persisted`
  - `component=forward_proxy event=startup_runtime_store_synced`

## Validation

- `cargo check`
- `cargo test --lib runtime_logging::tests::runtime_memory_helpers_parse_status_and_cgroup_values -- --nocapture`
- `cargo test --lib runtime_logging::tests::runtime_perf_scope_exposes_elapsed_and_memory_fields -- --nocapture`
- `cargo test --lib store::tests::perf_logs_are_info_level_and_include_memory_budget_fields -- --nocapture`
- `cargo test alerts_and_ha -- --nocapture`
- `cargo test log_catalog_and_dashboard_sse -- --nocapture`

## Notes

- 这张 spec 是 runtime 诊断、低内存和恢复模式的程序级合同真相源。
- 高频 `low_memory_protection_decision` 与 HA export/sync 信息应按状态跃迁或轻量采样输出，默认日志不再依赖密集重复 INFO 来做定位。
- HA peer-less export and baseline samples report `ack_lag=null`; normal summaries stay sampled per
  channel, while heavy outbox and memory snapshots remain reserved for slow, error, or threshold
  transition events.
- HA GC aggregate samples include the continuation delay and `next_retry_at`; the normal sampled
  path remains sufficient to diagnose deferred recovery without restoring per-slice WARN logs.

## Visual Evidence

PR: none

- evidence_note: This change only adjusts runtime diagnostics and scheduler recovery. Any future
  UI-affecting change must add current-SHA visual evidence before selecting it for a PR.
