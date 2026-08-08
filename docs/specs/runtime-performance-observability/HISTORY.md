# History：性能诊断日志与低内存稳定运行合同（#m2p6k）

## 关键演进

- SQLite 事务诊断收敛为 operation/class 聚合窗口，关联 pool/begin wait、持有时间、逻辑写入量与
  process/cgroup write-byte delta，同时禁止默认日志携带原始 SQL 或请求内容。

- 2026-08-02：完成低压 HA GC 恢复、候选分页与本地退避、业务调用紧凑缓存和惰性 footprint
  采集；默认日志保留状态跃迁和真实错误，避免以高频采样反向增压。

- 2026-07-31：内存诊断拆分 cgroup anon/file/swap 与进程 RSS 分类，普通 INFO 采集限制为五分钟
  一次；高频性能事件改为 DEBUG 与状态跃迁告警。

- 在线 HA outbox GC 的常规切片现以 DEBUG 记录 active SQL 耗时和续片延迟；每通道持久化的
  高水位、删除量与 ingress-minus-delete 估算用于低频趋势判断，避免通过 `COUNT(*)` 反向放大压力。

- 2026-06-23：创建程序级 spec，冻结“PR1 只补默认结构化性能日志与验证基建、PR2 再收口
  bounded-memory 与 `256MiB` 合同”的执行边界。
- 2026-06-23：runtime logging 扩展出 cgroup/进程组内存快照字段，默认日志开始覆盖 HA
  export/import/sync、dashboard snapshot、recent request reads、forward-proxy startup。
- 2026-06-23：owner-facing 重读路径开始输出 `low_memory_protection_decision` 结构化事件，
  先记录当前 verdict，作为 PR2 真实低内存退化动作的前置证据面。
- 2026-06-23：request/token logs perf 事件从误用的 `WARN` 口径收回到默认 `INFO`，避免把
  正常完成的诊断事件伪装成异常告警。

## 相关规范

- `docs/specs/edgeone-active-standby-ha/SPEC.md`
- `docs/specs/admin-dashboard-overview-performance/SPEC.md`
- `docs/specs/admin-recent-requests-performance-copy/SPEC.md`
- Corrected HA perf diagnostics so full-master export without a peer does not synthesize a zero
  ACK watermark; expensive outbox and memory sampling remains bounded to the existing slow/error
  and time-window triggers.

- Reconciliation and HA GC diagnostics now expose the causal boundary directly: main settlement starts before research sweep, local pressure has its own state, and normal GC progress is sampled per channel instead of logged per slice.
- The final review preserved that boundary with explicit request-start, settlement, and post-processing
  deadlines; local pressure metadata is replicated during HA takeover and normal GC diagnostics remain
  channel-sampled.

## Legacy Identity

- Legacy compatibility identity: `#m2p6k`.
