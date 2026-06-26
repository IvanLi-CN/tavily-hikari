# Runtime：性能诊断日志与低内存稳定运行合同（#m2p6k）

## 状态

- Status: active
- Created: 2026-06-23
- Last: 2026-06-26

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
- PR1 只补观测合同与验证基建，不改既有行为语义；PR2 再基于这些事件收口 bounded-memory 实现。

## Non-goals

- 不新增 Prometheus/OTel/owner-facing metrics 页面。
- 不在 PR1 改变 HA、dashboard、request logs、forward-proxy startup 的业务 contract。
- 不把秘密、header/body 明文或全量 SQL debug 输出进默认日志。

## Runtime Logging Contract

- 继续使用默认 `RUNTIME_LOG_FORMAT=json` + `stderr` 输出，保留 `text` fallback。
- 新增的性能事件必须使用现有 `tracing` 结构化字段，至少包含：
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
  - `outbox_row_count`
  - `outbox_oldest_age_secs`
  - `outbox_ack_lag`

## Required Perf Events

- HA:
  - `component=ha event=baseline_export_completed`
  - `component=ha event=events_export_completed`
  - `component=ha event=baseline_import_completed`
  - `component=ha event=events_import_completed`
  - `component=ha event=standby_sync_baseline_completed`
  - `component=ha event=standby_sync_events_completed`
- Dashboard / shared snapshot:
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

- 这张 spec 是 PR1/PR2 共用的程序级合同真相源。
- PR2 需要在此基础上把 low-memory 自动退化与 `256MiB` 稳定运行验收写成最终真相。
- 高频 `low_memory_protection_decision` 与 HA export/sync 信息应按状态跃迁或轻量采样输出，默认日志不再依赖密集重复 INFO 来做定位。
