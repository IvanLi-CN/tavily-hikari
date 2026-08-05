# Tavily Hikari 性能架构渐进加固实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Implementation: Ticket #485 SqliteRuntime seam 已完成，等待 child PR integration CI
- Lifecycle: active
- Delivery topology: aggregate-stack
- Integration branch: `prd/performance-architecture-hardening`

## Coverage / rollout summary

实现拆为 12 个 risk-gated child Ticket，按依赖波次进入 integration branch：

1. writable-tenure supervisor、持久 authority epoch 与 mixed-version capability gate（child
   #484 已实现，待合入 integration branch）
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

- Ticket #484 已建立 revision-owned cancellation、单 runtime promotion、持久 `demoting`
  恢复、单调 authority epoch fence，以及不改变 HA JSON response shape 的内部 capability header；
  legacy scheduler handles 和并发 remote-I/O jobs 已由 tenure runtime 持有并随 demotion 取消。
- 将这些 legacy workers 封装为独立 `MaintenanceRuntime`、业务写事务全面接入 authority epoch
  guard，以及其余 runtime ownership 收敛由后续 Ticket 完成。
- aggregate 验证、架构 checker、30 分钟 RSS 基准及 rollout 文档尚待完成。

## Related Changes

- `src/store/sqlite_runtime.rs` adds typed read, immediate-write, and admission operations with a
  bounded 250ms operation budget and cancellation/busy outcomes.
- `KeyStore` constructors create one `SqliteRuntime` from the existing pools and expose only
  compatibility handles while expand-contract callers migrate; pool sizes, busy timeouts,
  PRAGMAs, and transaction SQL remain unchanged.
- Ticket #484: revisioned writable-tenure supervisor and mixed-version capability gate.

## References

- `./SPEC.md`
- `./HISTORY.md`
