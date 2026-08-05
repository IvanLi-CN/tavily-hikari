# Tavily Hikari 性能架构渐进加固实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Implementation: Ticket #485 SqliteRuntime seam 已完成，等待 child PR integration CI
- Lifecycle: active
- Delivery topology: aggregate-stack
- Integration branch: `prd/performance-architecture-hardening`

## Coverage / rollout summary

实现拆为 12 个 risk-gated child Ticket，按依赖波次进入 integration branch：

1. writable-tenure supervisor、持久 authority epoch 与 mixed-version capability gate
2. SqliteRuntime seam
3. MaintenanceRuntime legacy adapter
4. RequestStatsPipeline
5. HaPeerObservationStore
6. per-channel HA GC 与 no-op outbox suppression
7. reconciliation work projection
8. AlertProjection expand/shadow
9. ReconciliationEngine cutover
10. 全部告警读取切换
11. DashboardReadModel/SSE cutover
12. 热路径旧 seam 删除与依赖门禁

## Remaining Gaps

- 所有 12 个 Ticket 尚未开始实现。
- aggregate 验证、架构 checker、30 分钟 RSS 基准及 rollout 文档尚待完成。

## Related Changes

- `src/store/sqlite_runtime.rs` adds typed read, immediate-write, and admission operations with a
  bounded 250ms operation budget and cancellation/busy outcomes.
- `KeyStore` constructors create one `SqliteRuntime` from the existing pools and expose only
  compatibility handles while expand-contract callers migrate; pool sizes, busy timeouts,
  PRAGMAs, and transaction SQL remain unchanged.

## References

- `./SPEC.md`
- `./HISTORY.md`
