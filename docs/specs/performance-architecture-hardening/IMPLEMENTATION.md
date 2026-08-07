# Tavily Hikari 性能架构渐进加固实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Implementation: Ticket #485 SqliteRuntime seam 已合入 integration branch；Ticket #484 已实现，等待
  child PR integration；Ticket #487 RequestStatsPipeline 已实现，等待 wave-gated integration；Ticket #489
  per-channel HA GC 已合入 integration branch；Ticket #490 reconciliation work projection 已合入
  integration branch
- Lifecycle: active
- Delivery topology: aggregate-stack
- Integration branch: `prd/performance-architecture-hardening`

## Coverage / rollout summary

实现拆为 12 个 risk-gated child Ticket，按依赖波次进入 integration branch：

1. writable-tenure supervisor、持久 authority epoch 与 mixed-version capability gate（child
   #484 已实现，待合入 integration branch）
2. SqliteRuntime seam（child #485 已合入 integration branch）
3. MaintenanceRuntime legacy adapter
4. RequestStatsPipeline（child #487）
5. HaPeerObservationStore
6. per-channel HA GC 与 no-op outbox suppression（Ticket #489 已合入 integration branch）
7. reconciliation work projection（Ticket #490 已合入 integration branch）
8. AlertProjection expand/shadow
9. ReconciliationEngine cutover
10. 全部告警读取切换
11. DashboardReadModel/SSE cutover
12. 热路径旧 seam 删除与依赖门禁

## Remaining Gaps

- Ticket #484 已建立 revision-owned cancellation、单 runtime promotion、持久 `demoting`
  恢复、单调 authority epoch fence，以及不改变 HA JSON response shape 的内部 capability header；
  legacy scheduler handles 和并发 remote-I/O jobs 已由 tenure runtime 持有并随 demotion 取消；外部业务
  request 在 tenure admission 内完成，demotion 排空已进入的 request 后才推进 epoch，authority SQLite
  写入的 pool 与 busy 等待总预算保持在 250ms 内。
- 将这些 legacy workers 封装为独立 `MaintenanceRuntime`、把 ingress fence 下沉为各业务写事务的细粒度
  authority epoch guard，以及其余 runtime ownership 收敛由后续 Ticket 完成。
- child #487 的 aggregate 生产形状 RSS 基准已完成（基准二进制 commit
  167b10dce69586c2badd4f5330a7c0bf93316045）：Linux x86_64 release binary，1,000,002
  条 workload 前 request logs，5 rps（HTTP API 60% / MCP 20% / admin 20%），20 条 SSE，
  5 分钟 warmup 后采样 30 分钟，共 360 个 5 秒 RSS 样本；RSS P50 为 220,480 KiB，
  P95 为 254,276 KiB，最大值为 258,020 KiB，低于 256 MiB 门槛。上游为本地
  mock_tavily，完整状态与 workload 证据记录在 Issue #487 handover 中。
- Ticket #489 已为 control、billing、runtime 建立独立 durable GC work、claim generation、eligibility、
  adaptive continuation 与 typed outcome；channel scheduled job 完成和 continuation 在同一 SQLite
  writer transaction 内提交，并为 wire-identical UPDATE 抑制 HA outbox 事件。

## Related Changes

- `src/store/sqlite_runtime.rs` adds typed read, immediate-write, and admission operations with a
  bounded 250ms operation budget and cancellation/busy outcomes.
- `KeyStore` constructors create one `SqliteRuntime` from the existing pools and expose only
  compatibility handles while expand-contract callers migrate; pool sizes, busy timeouts,
  PRAGMAs, and transaction SQL remain unchanged.
- Ticket #487: `RequestStatsPipeline` owns bounded pending rollups, snapshot metadata, paged
  dashboard backfill access, and typed request-stats storage operations; `scripts/check_request_stats_architecture.py`
  enforces the hot-path boundary and stable `(created_at, id)` pagination contract. Flush batches retain
  durable ids across retries and use an in-transaction marker to prevent additive replay after ambiguous
  commit outcomes.
- Quota sample pagination regression coverage is isolated in `src/tests/request_rollup_quota_paging.rs`;
  it uses a local stub upstream and the `BackendTime` seam so the 500-row cursor boundary remains
  deterministic without production calls.
- Ticket #484: revisioned writable-tenure supervisor and mixed-version capability gate.
- Ticket #489: durable per-channel HA GC work and unchanged-wire UPDATE suppression.
- Ticket #490: logical-window reconciliation work projection with bounded eligible pages, persistent
  recent/backlog cursors, stable per-key fair ranks, atomic reservations, bounded paged hydration,
  restartable legacy backfill, representative scheduling, and reservation-fenced settlement
  transitions.
- Aborted and budget-exhausted runs recover reservations and finish their claimed representative in
  one bounded transaction before scheduling the next continuation, so restart recovery cannot leave
  eligible work waiting for an unrelated scheduler tick.

## References

- `./SPEC.md`
- `./HISTORY.md`
