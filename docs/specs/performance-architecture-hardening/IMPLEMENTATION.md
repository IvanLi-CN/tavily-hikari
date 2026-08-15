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
  before pool acquisition. It reserves two actual idle or immediately allocatable pool slots for
  foreground work, defers bulk work when
  foreground arrival, recent busy/timeout signals, or idle capacity violate the runtime contract,
  and aggregates the decision by operation and workload class.
- HA GC, request-stats persistence, pressure rebuild, reconciliation projection, and Dashboard
  integrity use that admission boundary. Scheduler claim/finish/continuation remain short control
  transactions, so bulk backpressure cannot consume their control path or create an unbounded retry.
  A transient short-control completion leaves its generation-fenced row running for the periodic
  stale reaper; request-log GC persists its five-minute continuation in the same transaction as
  completion.
- HTTP manual enqueue uses a separate foreground operation budget. A transient HA GC enqueue failure
  returns `503` instead of claiming acceptance with a synthetic job id; the self-scheduling HA
  controller and worker wake provide the bounded recovery path without inventing a durable row.
- The short admission budgets are explicit runtime deadlines, not connection-level `PRAGMA busy_timeout` rewrites. A background request-stats admission commits at most four adaptive
  `25..250` logical-key transactions under one 50ms retry budget, then atomically returns its
  complete tail to the coalescer and reports `deferred` when needed. The deadline covers pool
  acquisition, `BEGIN IMMEDIATE`, writes, and commit; manual HA GC wakes reuse an
  existing durable representative rather than
  competing for a locked writer merely to promote queue metadata.
- Dashboard snapshot reads preserve a last-good value under admission or SQLite pressure. A cold
  shared loader is bounded per caller to one second without cancelling its in-flight build; startup
  gives that same loader a one-second head start before accepting external connections.
- The isolated 10-minute dual-database comparison records raw Dashboard and process RSS P95 values.
  Its relative regression gate has a 10ms Dashboard measurement floor and a 40MiB RSS noise band
  around the 10% threshold so controlled restart and allocator variation do not reject the same
  candidate; the separate 30-minute release RSS benchmark remains the `<=256MiB` SLO authority.

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
- 五分钟 watchdog 以 `SqliteRuntime` 的短 control read 复查三条 channel state 的 observation age。已过期和从未
  observation 的 channel 都加入当前 slice 的公平 probe，即使另一个 channel 仍有 debt；只有 probe 确认仍有工作时才
  写回 durable pending mask，因此空 channel 不会形成一秒循环。这个 state-only discovery 不读取 outbox；实际
  control/billing/runtime 查询仍逐片经过 bulk admission 和持久化轮转。
- `ReconciliationEngine` 将 work 完成归类为 `settled`、`no_adjustment`、`upstream_429`、
  `transport_failure`、`semantic_failure` 或 `local_pressure`。`no_adjustment` 是当前 usage generation 的
  terminal result，只有新 usage 才重新投影 work；本地压力、429 与其他失败状态彼此独立持久化。
- 若对账的 local preparation 被 SQLite bulk admission 拒绝，engine 返回 typed deferred outcome；scheduler 用短
  control transaction 原子 finish 并在 30 秒后续排同一 durable representative，不能把零工作当作成功完成。

## Related Changes

- None

## References

- `./SPEC.md`
- `./HISTORY.md`
