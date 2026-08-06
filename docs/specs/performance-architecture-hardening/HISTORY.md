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
- per-channel HA GC 使用独立 durable eligibility 与 generation lease；完成事务同时更新 typed
  outcome、adaptive channel state、scheduled job 与 continuation，避免 control 延迟阻塞 billing 或
  runtime，并让旧 claim 在接管后无法覆盖新状态。
- HA outbox UPDATE trigger 以 wire JSON 比较 old/new payload；只有有效变化写入一条兼容事件，保持
  旧 reader、retention、ACK 与 `410 -> baseline` 语义不变。

## Key Reasons / Replacements

- Ticket #485 establishes the first storage ownership seam: `SqliteRuntime` owns canonical pool
  handles, admission, and the 250ms bounded operation contract while legacy `KeyStore` callers use
  explicit compatibility adapters during rollout.

- 反复局部优化未能稳定消除性能回归，因为生命周期、存储和读取边界仍由浅层 facade 共享。
- durable per-channel work 替代单一 HA GC representative，避免一个 channel 的延迟阻塞全部排债。
- durable projections 与共享 snapshots 替代请求线程上的重复聚合和 flush-on-read。
- 窄 runtime 接口和 architecture checker 替代依赖约定，防止 raw pool/coalescer 再次泄漏。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
