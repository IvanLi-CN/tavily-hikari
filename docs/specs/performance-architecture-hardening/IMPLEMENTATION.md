# Tavily Hikari 性能架构渐进加固实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Implementation: Ticket #485 SqliteRuntime seam 已合入 integration branch；Ticket #484 已实现，等待
  child PR integration；Ticket #490 reconciliation work projection 已在 child branch 实现，等待
  wave-gated integration
- Lifecycle: active
- Delivery topology: aggregate-stack
- Integration branch: `prd/performance-architecture-hardening`

## Coverage / rollout summary

实现拆为 12 个 risk-gated child Ticket，按依赖波次进入 integration branch：

1. writable-tenure supervisor、持久 authority epoch 与 mixed-version capability gate（child
   #484 已实现，待合入 integration branch）
2. SqliteRuntime seam（child #485 已合入 integration branch）
3. MaintenanceRuntime legacy adapter
4. RequestStatsPipeline
5. HaPeerObservationStore
6. per-channel HA GC 与 no-op outbox suppression
7. reconciliation work projection（Ticket #490 已实现，等待 wave-gated integration）
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
- aggregate 验证、架构 checker、30 分钟 RSS 基准及 rollout 文档尚待完成。

## Related Changes

- `src/store/sqlite_runtime.rs` adds typed read, immediate-write, and admission operations with a
  bounded 250ms operation budget and cancellation/busy outcomes.
- `KeyStore` constructors create one `SqliteRuntime` from the existing pools and expose only
  compatibility handles while expand-contract callers migrate; pool sizes, busy timeouts,
  PRAGMAs, and transaction SQL remain unchanged.
- Ticket #484: revisioned writable-tenure supervisor and mixed-version capability gate.
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
