# 本地请求与维护成本边界：实现覆盖

## Current Status

- Lifecycle: active
- Implementation: 七项性能机制已在父分支落地；当前头的正式 A8、升级/恢复与 HA 证据仍待收口。
- 当前基线：父分支 `prd/101-performance-hardening`，commit `699a065cfe27deabb30d401b8f9de57f9dd0bcff`，基线 `b1bb1043823b9260b18389854e6b90b2cdedbd70`。本地、Agent VM 与 101 不执行性能或对比负载。

## Implementation Coverage

| 要求               | 主要实现归属                                                                                       | 已确认缺口                                                                                    |
| ------------------ | -------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| REQ-LOCAL-PERF-001 | `src/observability.rs`、HTTP/MCP 请求处理、SQLite defer 分类                                       | 需在 GitHub-hosted mock A8 绑定同 cohort 验证完整请求阶段                                     |
| REQ-LOCAL-PERF-002 | `src/store/key_store_alert_projection.rs`、core ledger migration                                   | 需保留当前头的应用 SQLite EQP/VM-work 与升级墙钟证据                                          |
| REQ-LOCAL-PERF-003 | `src/store/key_store_system_settings.rs`、meta decoder                                             | 需在当前头复核 29→1 与成功 search 87→3 的配置 SQL 计数                                        |
| REQ-LOCAL-PERF-004 | `src/store/sqlite_runtime.rs`、projection 与 integrity source reads                                | 主路径已使用 native read session；仍需完成当前头的取消、清理失败与恢复证据                    |
| REQ-LOCAL-PERF-005 | `src/store/key_store_dashboard_rollup_integrity.rs`、durable eligibility 与 scheduled continuation | 持久 unchanged-range 五分钟门禁已落地；需完成跨进程竞争与 accepted progress 证据              |
| REQ-LOCAL-PERF-006 | `src/store/key_store_upstream_reconciliation.rs`、MaintenanceRuntime、scheduler/watchdog           | durable receipt 后的唯一后台 owner 与 generation 保留已落地；需完成崩溃窗口和 HA 生命周期证据 |
| REQ-LOCAL-PERF-007 | ledger、SQLite runtime tests、业务 mock 与性能 fixtures                                            | 当前头 A8、升级、恢复和 HA 的正式证据仍未收口                                                 |

## Verification and Rollout

- 合成复现使用 Python SQLite 3.45.3，生产只读探针使用 SQLite 3.53.4；当前应用依赖绑定 SQLite 3.44.0。正式 SQL、planner 和 native cancellation 证据必须由应用实际 SQLite 产生。
- 当前常见成功 search 的 settings SQL 验收基线为每次 29→1、三处读取 87→3；仅统计这些 settings lookup，不包含其它业务 SQL。
- 生产 continuation 只读样本约 28ms / 140.2 万 VM steps；配对的真实请求延迟尚未测得，不能声称该项能消除秒级总残差。
- 原生 AlertProjection、integrity baseline 与 source-page 读预算已接入 SqliteRuntime-owned session；超时必须在 native progress boundary 收尾并保持连接清理。
- million-row index 构建与 blocked-range state 的 migration 仍需单独记录 writer 阻塞、磁盘和 readiness。v46 ledger 已存在而对象缺失时 warm reopen 必须 fail closed，不得静默重建；旧二进制对未知 ledger 的拒绝使直接降级不成立。
- 现有 `observability.rs`、`sqlite_runtime_tests.rs`、settings/HTTP billing 测试和 reconciliation generation/claim 测试可扩展复用。测试选择遵守 [testing.md](../../agents/testing.md)；文件并发、集成和性能负载使用会话 Agent VM 与 mock/sandbox upstream。
- 对照负载固定 200ms mock、小 payload、usage、subject/token 分布、配置、metrics 开关、资源限制和预热方式；每次只改变一个机制，最后再比较累计行为。

## Remaining Gaps

计费锁的线上耗时占比尚需配对观测；正式 A8 只在 GitHub-hosted mock runner 执行。当前文档不表示已部署 101 或已改变生产。

## References

- [Requirements](./SPEC.md)
- [History](./HISTORY.md)
