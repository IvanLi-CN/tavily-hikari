# Tavily Hikari 性能架构渐进加固实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Implementation: 当前集成候选上的增量收敛
- Lifecycle: active
- Delivery topology: initiative aggregate
- Integration branch: `prd/performance-debt-recovery`

## Coverage / rollout summary

本轮已落地的边界包括：

1. `HaPeerObservationStore` 的后台探测与管理员缓存读路径
2. `HaGcController` 的 per-channel HA GC eligibility、claim generation、最早唤醒与公平轮转
3. `ReconciliationEngine` durable work projection、两秒主结算预算与 typed terminal outcome
4. versioned additive schema migration ledger

## SQLite admission containment

- `SqliteRuntime` is now instance-owned by `KeyStore` and admits a single bulk operation only
  before pool acquisition. It reserves two pool slots for foreground work, defers bulk work when
  foreground arrival, recent busy/timeout signals, or idle capacity violate the runtime contract,
  and aggregates the decision by operation and workload class.
- HA GC, request-stats persistence, pressure rebuild, reconciliation projection, and Dashboard
  integrity use that admission boundary. Scheduler claim/finish/continuation remain short control
  transactions, so bulk backpressure cannot consume their control path or create an unbounded retry.
- HTTP manual enqueue uses a separate foreground operation budget. A transient HA GC enqueue failure
  returns the existing trigger-response shape as `202/deferred` with `jobId=0`, which means no durable
  row was created; the self-scheduling HA controller and worker wake provide the bounded recovery path.
- The short admission budgets are explicit runtime deadlines, not connection-level `PRAGMA busy_timeout` rewrites. A background request-stats admission commits at most four adaptive
  `25..250` logical-key transactions under one 50ms retry budget, then atomically returns its
  complete tail to the coalescer and reports `deferred` when needed; manual HA GC wakes reuse an
  existing durable representative rather than
  competing for a locked writer merely to promote queue metadata.
- Dashboard snapshot reads preserve a last-good value under admission or SQLite pressure. A cold
  shared loader is bounded per caller to one second without cancelling its in-flight build; startup
  gives that same loader a one-second head start before accepting external connections.

## Remaining Gaps

- `SqliteRuntime` 的首个 containment slice 已进入交付：取消安全 read/immediate guard、读路径禁止
  request-stats flush、事务源码门禁与低频 workload 归因。
- DashboardReadModel、AlertProjection、完整 MaintenanceRuntime 与 HA writable-tenure 生命周期仍是后续架构工作，未在本变更中伪装为已完成。
- 101 双库只读快照上的 baseline/candidate production-shape 对比、全量质量门禁和 aggregate PR review
  是交付前硬门禁。

## Durable recovery convergence

- `HaGcController` 以 `ha_outbox_gc_channel_state` 作为 control、billing、runtime 的唯一可运行时间、
  claim generation、batch、legacy cursor、进展与 defer 真相。scheduler 仅消费 controller 给出的最早
  wake；一个 channel 的 slow、busy 或 legacy defer 不会再把其他 eligible channel 推迟到同一全局延迟。
- 在线 GC 始终只持有一个 writer slice：每片一个 channel、`25..250` 自适应 batch、单 SQL `50ms` 目标和
  一秒上限。低压连续五分钟后采用一秒 continuation；正常进展保持五秒，前台压力、busy 或慢 SQL 只让
  受影响 channel 退让 30 秒。
- 当 pending mask 清空后，五分钟 watchdog 以 `SqliteRuntime` 的短 control read 复查三条 channel state
  的 observation age。这个 state-only discovery 可重新启动漏标的历史债务，但不读取 outbox；实际
  control/billing/runtime 查询仍逐片经过 bulk admission 和持久化轮转。
- `ReconciliationEngine` 将 work 完成归类为 `settled`、`no_adjustment`、`upstream_429`、
  `transport_failure`、`semantic_failure` 或 `local_pressure`。`no_adjustment` 是当前 usage generation 的
  terminal result，只有新 usage 才重新投影 work；本地压力、429 与其他失败状态彼此独立持久化。

## Related Changes

- None

## References

- `./SPEC.md`
- `./HISTORY.md`
