# Tavily Hikari 性能架构渐进加固演进历史

> 这里记录影响架构理解的关键演进原因；当前规范仍以 `./SPEC.md` 为准。

## Decision Trace

- 采用 aggregate-stack，由 12 个独立 Codex task 和 risk-gated child PR 渐进交付。
- 保留单体 + SQLite，以 runtime ownership、durable work 和 read model 收敛热路径。
- 使用 expand-contract 保持滚动升级兼容，混跑期禁止 HA 角色切换。
- child task 固定模型与 reasoning effort，恢复和 PR 修复不得擅自换档。
- writable authority 以单调 SQLite epoch 和 `standby|writable|demoting` phase 持久化；demotion
  先取消 revision token，再提交新 epoch，重启遇到 `demoting` 时保持 fail closed。
- `writable_tenure_v1` 通过内部 HA response header 扩展，避免改变 public/admin JSON；planned
  transition 要求全员能力，新旧混跑期间因此 fail closed，force emergency 路径继续使用既有防脑裂
  检查与审计。
- writable tenure 持有 legacy scheduler handles 与 worker 内并发 remote-I/O task；demotion 的
  revision cancellation 因而能够在 epoch 推进前终止旧 runtime，而后续 Ticket 再将该适配层封装为
  独立 `MaintenanceRuntime`。
- 外部业务 request 在 writable tenure admission 内运行；demotion 取得独占 admission、排空已进入的
  request 后再推进 SQLite epoch。authority 写使用独立短等待预算，writer contention 不会继承主 pool
  的 5 秒 busy timeout。

## Key Reasons / Replacements

- Ticket #485 establishes the first storage ownership seam: `SqliteRuntime` owns canonical pool
  handles, admission, and the 250ms bounded operation contract while legacy `KeyStore` callers use
  explicit compatibility adapters during rollout.

- 反复局部优化未能稳定消除性能回归，因为生命周期、存储和读取边界仍由浅层 facade 共享。
- durable per-channel work 替代单一 HA GC representative，避免一个 channel 的延迟阻塞全部排债。
- durable projections 与共享 snapshots 替代请求线程上的重复聚合和 flush-on-read。
- 窄 runtime 接口和 architecture checker 替代依赖约定，防止 raw pool/coalescer 再次泄漏。
- reconciliation work projection 以 `token_id + period_code` 持久化逻辑结算窗口；recent/backlog
  各自保存公平 cursor，candidate page 在 representative ranking 前做有界 overfetch，并通过
  `SqliteRuntime` 的 bounded immediate claim 保持 12/8 配额与首个远端尝试预算。
- work reservation、retry、settlement 和 billing adjustment 在同一 SQLite 写事务中以 reservation
  fence 排序；失效 worker 只能放弃结果，不能覆盖新 reservation 或重复改变账务真值。窗口内完整
  key hydration 仍来自选中窗口的 usage rows，`scheduling_key_id` 只承担公平代表调度。
- usage projection 会 enqueue 唯一 reconciliation representative job；启动恢复 stale job 并唤醒
  maintenance runtime，使新窗口不必等待 scheduler tick 才进入处理。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
