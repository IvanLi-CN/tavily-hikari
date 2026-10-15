# 本地请求与维护成本边界：实现覆盖

## Current Status

- Lifecycle: active
- Implementation: 要求已确认，新增行为尚未实现。
- 当前诊断基线：backend 0.86.0，commit `b1bb1043823b9260b18389854e6b90b2cdedbd70`。生产只读诊断与合成 SQL 复现不构成完整后端验收。

## Implementation Coverage

| 要求               | 主要实现归属                                                                                       | 已确认缺口                                                                               |
| ------------------ | -------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| REQ-LOCAL-PERF-001 | `src/observability.rs`、HTTP/MCP 请求处理、SQLite defer 分类                                       | 缺少同请求配对的锁等待、落库和尾部阶段；readiness 被归入 deadline                        |
| REQ-LOCAL-PERF-002 | `src/store/key_store_alert_projection.rs`、core ledger migration                                   | scheduled-job fence 扫描约 132.6 万行；仅加索引不能保证后段分页有界                      |
| REQ-LOCAL-PERF-003 | `src/store/key_store_system_settings.rs`、meta decoder                                             | 每次读取 27 个直接 lookup 和 2 个子函数 lookup；常见成功 search 三处合计 87 条           |
| REQ-LOCAL-PERF-004 | `src/store/sqlite_runtime.rs`、projection 与 integrity source reads                                | AlertProjection 尚无 native execution budget，部分 integrity 查询只由 async timeout 包围 |
| REQ-LOCAL-PERF-005 | `src/store/key_store_dashboard_rollup_integrity.rs`、durable eligibility 与 scheduled continuation | 历史 baseline 缺口在 source-page 之前阻塞，尚无持久 unchanged-range 五分钟门禁           |
| REQ-LOCAL-PERF-006 | `src/store/key_store_upstream_reconciliation.rs`、MaintenanceRuntime、scheduler/watchdog           | usage commit 后同步计算全局 continuation；只 notify 现有 worker 不能创建任务             |
| REQ-LOCAL-PERF-007 | ledger、SQLite runtime tests、业务 mock 与性能 fixtures                                            | native 后端 SQL、完整 A/B、升级和恢复验收尚未执行                                        |

## Verification and Rollout

- 合成复现使用 Python SQLite 3.45.3，生产只读探针使用 SQLite 3.53.4；当前应用依赖绑定 SQLite 3.44.0。正式 SQL、planner 和 native cancellation 证据必须由应用实际 SQLite 产生。
- 当前常见成功 search 的 settings SQL 验收基线为每次 29→1、三处读取 87→3；仅统计这些 settings lookup，不包含其它业务 SQL。
- 生产 continuation 只读样本约 28ms / 140.2 万 VM steps；配对的真实请求延迟尚未测得，不能声称该项能消除秒级总残差。
- 原生 AlertProjection 预算是新增合同；应先确认来源 fence/page 已有正常进展，再用预算约束异常读。来源阻塞退避需使用原生有界 probe 和准确原因分类。
- million-row index 构建与 blocked-range state 的 migration 应单独测 writer 阻塞、磁盘和 readiness。实施时按最终 ledger 基线分配序号，不改已应用 checksum；旧二进制对未知 ledger 的拒绝使直接降级不成立。
- 现有 `observability.rs`、`sqlite_runtime_tests.rs`、settings/HTTP billing 测试和 reconciliation generation/claim 测试可扩展复用。测试选择遵守 [testing.md](../../agents/testing.md)；文件并发、集成和性能负载使用会话 Agent VM 与 mock/sandbox upstream。
- 对照负载固定 200ms mock、小 payload、usage、subject/token 分布、配置、metrics 开关、资源限制和预热方式；每次只改变一个机制，最后再比较累计行为。

## Remaining Gaps

所有新增要求的实现与对应 VER 验收仍待交付；计费锁的线上耗时占比尚需配对观测。当前文档不表示已修复、已创建 PR 或已变更生产。

## References

- [Requirements](./SPEC.md)
- [History](./HISTORY.md)
