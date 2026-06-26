# Implementation：性能诊断日志与低内存稳定运行合同（#m2p6k）

## 当前状态

- 状态：部分完成（PR1 已落地，PR2 待实现）
- 最近更新：2026-06-26

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

- PR2 仍需把 HA baseline/events export/import 与 standby sync 的 bounded-memory 行为彻底收口；
  PR1 只补可观测性，没有改变这些路径的资源使用合同。
- PR2 仍需把真正的 low-memory 自动退化逻辑接到 owner-facing 重读路径上，并把
  `256MiB` 进程组合同做成可验收 harness。
- 目前 `low_memory_protection_decision` 事件只记录当前 verdict，不代表已经具备真正的
  `503 low_memory_protection` 保护行为。
- 当前只为 HA outbox 增加观测字段，没有修改 retention、GC、repair 或 compaction 行为；这些仍属于后续独立治理范围。

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
