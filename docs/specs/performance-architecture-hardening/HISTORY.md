# Tavily Hikari 性能架构渐进加固演进历史

> 这里记录影响架构理解的关键演进原因；当前规范仍以 `./SPEC.md` 为准。

## Decision Trace

- 采用 aggregate-stack，由 12 个独立 Codex task 和 risk-gated child PR 渐进交付。
- 保留单体 + SQLite，以 runtime ownership、durable work 和 read model 收敛热路径。
- 使用 expand-contract 保持滚动升级兼容，混跑期禁止 HA 角色切换。
- child task 固定模型与 reasoning effort，恢复和 PR 修复不得擅自换档。
- `SqliteRuntime` 先以独立止血变更建立强制事务边界与 read-never-writes 合同；后续架构 Ticket 基于该
  边界继续迁移，不等待固定线上观察窗。

## Key Reasons / Replacements

- 反复局部优化未能稳定消除性能回归，因为生命周期、存储和读取边界仍由浅层 facade 共享。
- durable per-channel work 替代单一 HA GC representative，避免一个 channel 的延迟阻塞全部排债。
- durable projections 与共享 snapshots 替代请求线程上的重复聚合和 flush-on-read。
- 窄 runtime 接口和 architecture checker 替代依赖约定，防止 raw pool/coalescer 再次泄漏。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
