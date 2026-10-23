# 本地请求与维护成本边界

## Context and Scope

本主题定义可验证的本地工作成本合同：业务请求的配对阶段耗时、配置读取成本、告警来源查询成本、原生维护读预算、来源基线不足时的工作资格，以及 durable usage 提交后的调度边界。它补充现有性能架构的模块所有权合同，约束每次实际工作及无进展重试的成本；原有账务、HA、Research、admission 和删除保护合同继续由各自主题管理。

目标是让本地延迟具有可靠归因，并让维护成本受页大小、原生预算和实际工作资格约束。系统不得把任务执行次数、连接持有时间或无法配对的聚合差值解释为账务进展、独占 writer 时间或单个请求的本地延迟。

范围不包括 credit reservation、解除同账户计费串行、扩充连接池、迁移数据库、生产清理或部署操作。独立 observability 文件的 journal、checkpoint、备份与只读行为由 [observability 存储合同](../observability-sqlite-storage/SPEC.md) 管理。

## Terms and Interfaces

- 领域术语采用根目录 [CONTEXT.md](../../../CONTEXT.md) 中的 foreground truth、derived observability、recovery progress、bounded recovery turn 和 source-baseline-blocked work。
- 观测通过现有进程内有界 recorder 与内部 exporter 提供；label key 继续采用 [monitoring 合同](../monitoring-dashboard/SPEC.md) 的 allowlist。公开聚合 metrics API 不提供这些内部诊断。
- settings load 指一次完整 SystemSettings 读取。durable usage receipt 指对应 SQLite 业务提交已经成功；调度提示不构成该 receipt。
- blocked range 的原因、下一次探测时刻与 accepted progress 通过现有管理员状态接口和脱敏日志可见，不要求新建管理页面。

## Requirements

### REQ-LOCAL-PERF-001：配对的请求耗时与原因分类

系统 MUST 为 HTTP 和 MCP 业务请求记录一个请求作用域，从业务处理开始到 Response 创建或终止，累计互斥的叶阶段：configuration、billing_lock_wait、preflight、key_selection、upstream、response_body、persistence、settlement、reconciliation_usage、reconciliation_schedule 和 other。重复进入同一阶段应累计；嵌套计时必须暂停父阶段。尚未命名的工作必须有明确归属，不能全部记为锁等待或 SQLite。

每个作用域 MUST 以一个有界 terminal 事件发布 total 和完整的阶段值；未进入的阶段为零，全部使用同一个最终 outcome 和业务操作类别。取消或无 Response 的终止使用固定类别，不产生半份观测；队列溢出必须丢弃整个事件。MCP 业务调用与协议控制消息必须通过有限 operation/method 类别区分，任意工具名只可映射到固定默认类别。旧 total 和旧 phase 的语义必须保留，不计入 Response 创建后的 SSE 传输。

系统 MUST 区分 coverage/readiness 阻塞、来源基线不足、实际 native query deadline、pool pressure 和 writer contention；原因不得由一个通用 deadline 类掩盖。计量不得新增 SQLite 写入，不得把 token、account、key、request id、正文、SQL、参数或任意错误文本放进 label 或 payload。

### REQ-LOCAL-PERF-002：真正有界的告警来源定位

scheduled-job 告警来源的 fence 和 key-page MUST 使用与 `LOWER(TRIM(status)) IN ('error', 'failed')` 及 `COALESCE(finished_at, started_at, queued_at)` 匹配的索引范围定位，保留 timestamp fallback、状态规范化、retention、cursor 排除、fence 包含、id 顺序和有界 hydrate。仅添加索引却继续扫描已翻过的历史前缀不满足要求。

page MUST 同时约束 cursor 同时间戳的 id 下界、严格中间时间段及 fence 同时间戳的 id 上界。可顺序读取最多三份有界候选页；同时间戳范围不得发生历史规模排序，固定小集合合并可接受。页成本不得随总历史、已翻过的前缀、同时间戳旧前缀或固定 fence 后追加量线性增长。

索引 MUST 通过生产实际 ledger migration 路径建立，覆盖已有库升级与各 HA 节点本地初始化；测试 bootstrap 不替代 warm upgrade。migration 后的 warm verification 必须只读且发现缺失结构。

### REQ-LOCAL-PERF-003：一次 settings load 一次 SQL

每次完整 settings load MUST 通过一条 SELECT 读取所有必需的 meta key，再以内存解析构建 SystemSettings；不得执行逐 key 的顺序查询。默认值、env fallback、非法值处理、持久 override、JSON 和百分比/retention 规范化、trusted IP 与已有 retention-cache 失效规则必须等价。

现有调用边界 MUST 保留实时读取语义，成功 settings PUT 后下一个请求应读取新值。本合同不引入跨请求 TTL cache 或跨调用配置快照；HTTP/MCP 的 header、路由、body、outcome 和 credits 必须等价。

### REQ-LOCAL-PERF-004：停止原生执行的维护读预算

AlertProjection 的相关来源 SELECT MUST 通过 SqliteRuntime-owned read session 执行，每条具有 250ms native execution 预算，pool acquire 与 busy wait 各有独立的 100ms 上限。integrity 的 baseline COUNT/SUM 与相关 source-page SELECT 必须执行既有 150ms native read 预算，并受所在 turn 更早的剩余预算约束。读取必须发生在短写事务之外；异步 future timeout 不能替代 native 执行中止。

到期或取消 MUST 在 native progress boundary 停止查询，并确认 worker 已结束后再恢复连接。progress handler 和临时连接配置必须被清理；不能确认清理时应丢弃物理连接。被中止或尚未接受的页不得提交 counts、cursor、fence 或 generation，下一轮能够重试相同页。

已提交页 MUST 按实际提交结果记录进展，不能因外层等待结束重复应用；已经开始的原子写事务必须先完成 commit/rollback，不得被读预算取消或当成未提交 work 回灌。低压正常数据应取得 accepted progress，永久 timeout 不满足成本合同；前台容量和 bulk permit 的保护规则继续适用。

### REQ-LOCAL-PERF-005：来源基线不足时的持久资格门禁

系统 MUST 按受阻 day/range 持久化 blocked reason、next_probe_at 和最近一次已计算的来源证据，保留已有 work cursor/counts。证据未变化时，两次 baseline probe 至少相隔 300 秒；启动 reset、manual enqueue、watchdog wake 或跨进程读取不能绕过内层资格门禁。外层 scheduled-job available_at 不能替代 range eligibility。

未到期 blocked range MUST 被工作选择跳过，其它 eligible range、hot tail 和维护 class 仍可接受进展。等待期间不得持有连接、reader、bulk permit 或不必要的 coordinator ticket。只有该范围的来源、封存或已删贡献证据明确变化，才允许对一个新 evidence revision 提前探测一次；缺少廉价范围信号时只采用显式修复提示和定时探测。不得用全局 max(id)、其它范围流量、每秒 COUNT 或每个前台请求新增观测写入来绕过退避。

探测 MUST 遵守原生预算和 claim fence。仍然不足时继续退避并保留 GC 的来源/封存删除保护；证据补齐后恢复正常 cursor 推进。blocked 或仅 probe 的执行不构成 accepted recovery progress，也不能重置有效进展计量；尚未到期的 blocked work 不属于该时刻可推进的 recovery debt。

### REQ-LOCAL-PERF-006：业务提交与全局调度分离

业务请求 MUST 同步提交 usage、Research 及触发器产生的 work generation，成功后取得 durable receipt，并向一个 MaintenanceRuntime-owned coalescing scheduler 发送有界提示。请求不得执行全局 reconciliation continuation SELECT，不得每请求 spawn task，不得以内存队列替代 usage 持久化。

唯一 scheduler MUST 通过现有 bounded reconciliation read session 和正确 admission 计算全局 continuation，在短 control 事务中 enqueue 成功后才通知执行 worker。bulk SELECT 不得伪装成 maintenance_control；queued/running representative 最多一个。运行前消耗 dirty/generation，运行中新增提示必须保留到下一轮；enqueue 失败保留 work 并 bounded defer。

健康资源下合并窗口 MUST 不超过一秒；startup 和既有三十秒 watchdog 从 durable state 恢复丢失提示。争用、pause 或 disabled 状态可延后调度，但必须保留 durable work 和显式 defer，watchdog 周期不构成无条件完成 SLA。compare、active、active_paused、activation boundary、HA authority/tenure、shutdown 和 Research 独立 drain 的现有合同必须保留。

### REQ-LOCAL-PERF-007：可解释的性能与兼容验收

SQL 正式验收 MUST 使用应用实际绑定的 SQLite，记录 query plan、VM work、版本和 fixture 分布；索引验证不得依靠 INDEXED BY 提示掩盖实际 planner 行为。请求性能验收必须使用 mock/sandbox upstream、固定配置与成对重复试验，记录同一 cohort 的样本量、均值、P95、吞吐、异常和 accepted progress，不得相减不同分布的 P95 或用单 SQL 倍数预测整条请求收益。

可判定的稳定无关场景 P95 回归 MUST 不超过 5%；样本不足或噪声无法判定时应标记缺验证，不能宣称通过。迁移构建的墙钟、writer 阻塞、磁盘增量和就绪耗时必须与稳态性能分开记录。

新增 ledger/state 变更 MUST 覆盖冷库、旧库 warm upgrade、重复升级、warm reopen 与缺失结构错误，不修改已应用 migration checksum。升级后的回退必须使用认识新 ledger 的兼容构建或受控向前修复，不得以旧二进制直接启动未知 ledger，或用旧快照覆盖其后产生的业务账务；派生 schema/index 不引入 HA 业务 outbox 或文件复制协议。

## Verification

### VER-LOCAL-PERF-001

- covers: `REQ-LOCAL-PERF-001`
- 方法：mock upstream 控制同账户排队与不同账户请求，覆盖早退、quota 拒绝、失败、billing deferred、MCP control/business、取消、disabled 和整份 overflow。
- 通过条件：每个被接受的业务观测仅发布一次 total 和一次各阶段值，阶段相加在计时精度内等于 total；第二个同账户请求准确累计锁等待，不同账户不共享该 subject 的等待。固定标签、旧指标语义和业务结果等价；readiness、baseline、native deadline、pool 与 writer 原因可分别识别。

### VER-LOCAL-PERF-002

- covers: `REQ-LOCAL-PERF-002`
- 方法：实际后端 SQLite 的无提示 EQP/VM-work 对照，覆盖起点、中段、末尾、全同时间戳、空页、nullable timestamp、混合状态、retention、不同 limit，以及 fence 后同秒大量追加。
- 通过条件：结果与参考查询逐页相同；代表 fixture 的 fence VM work 至少降低 99%，page work 随 page size 有界且不随历史前缀或 fence 后增量线性增长。最多固定小集合排序，不发生历史规模排序。

### VER-LOCAL-PERF-003

- covers: `REQ-LOCAL-PERF-003`
- 方法：按 settings load 统计 SQL，比较空配置、缺失/非法值、env fallback、持久 override、各开关、JSON、retention 和成功 PUT 的下一请求。
- 通过条件：每次完整读取仅一条 SQL；HTTP/MCP header、路由、body、outcome、credits 和 cache 失效行为与基线相同。

### VER-LOCAL-PERF-004

- covers: `REQ-LOCAL-PERF-004`
- 方法：昂贵 SELECT、busy、pool pressure、取消和清理失败的 native session 测试，以及低压正常 source-page 验证。
- 通过条件：查询在到期后的 native progress check 停止；返回/归还连接前已确认 worker 结束，连接安全复用或明确 discard。中止页无 durable 进展且可重试，已提交页不重复应用；正常页能够提交，原子写事务完成边界正确。

### VER-LOCAL-PERF-005

- covers: `REQ-LOCAL-PERF-005`
- 方法：manual BackendTime、进程重开、manual/watchdog 连续唤醒、跨进程竞争、无关流量和范围修复 evidence revision。
- 通过条件：首次阻塞后 299 秒内不执行 baseline scan，300 秒后至多一个有界 probe；每个新范围证据最多一次提前探测，仍不足则重新退避。其它工作能够推进，GC guard 不放宽；状态提供 reason、next_probe_at 和实际 accepted progress。

### VER-LOCAL-PERF-006

- covers: `REQ-LOCAL-PERF-006`
- 方法：覆盖 commit 前失败、commit 后提示前退出、enqueue 后 notify 前退出、enqueue 失败、运行中新增 usage、stale claim、新 generation，以及 pause/HA/disabled/shutdown 生命周期。
- 通过条件：请求内全局 continuation SQL 为零，增加 completed history 或无关 pending work 不增加请求尾部调度工作；durable usage 不丢失、代表唯一、运行中新增 work 不被吞。健康资源合并窗口至多一秒，丢失提示由 startup/watchdog 恢复，响应和 credits 等价。

### VER-LOCAL-PERF-007

- covers: `REQ-LOCAL-PERF-001`, `REQ-LOCAL-PERF-002`, `REQ-LOCAL-PERF-003`, `REQ-LOCAL-PERF-004`, `REQ-LOCAL-PERF-005`, `REQ-LOCAL-PERF-006`, `REQ-LOCAL-PERF-007`
- 方法：单账户串行、多账户并发 4/8、同账户排队、低压允许 admission 的维护，以及超过普通 5rps 门槛的受控 aged recovery；另用百万行合成库验证 migration 和各 reopen 路径。
- 通过条件：业务结果和删除保护等价，逐项成本证据可复现，稳定无关场景 P95 回归不超过 5%。一次性构建成本独立报告；旧 ledger 升级和各节点本地治理可验证，回退不会覆盖新业务事实。

## Related ADRs

- [ADR 0002: Scoped SQLite and Remote Admission](../../adr/0002-scoped-sqlite-and-remote-admission.md)
- [ADR 0004: Independent Research Drain](../../adr/0004-reconciliation-research-drain.md)
- [ADR 0005: Fair Maintenance Admission](../../adr/0005-fair-sqlite-maintenance-admission.md)
- [ADR 0008: Bounded Request Statistics Recovery](../../adr/0008-bounded-request-statistics-recovery.md)
- [ADR 0009: Runtime Metrics Ownership](../../adr/0009-runtime-metrics-ownership-boundary.md)
- [ADR 0011: Reconciliation Scheduling Owner](../../adr/0011-reconciliation-scheduling-owner.md)

## References

- [Implementation coverage](./IMPLEMENTATION.md)
- [History and compatibility](./HISTORY.md)
- [Performance architecture ownership](../performance-architecture-hardening/SPEC.md)
- [SQLite maintenance and deletion guards](../sqlite-write-lock-hardening/SPEC.md)
- [Runtime metrics catalog](../monitoring-dashboard/METRICS.md)
- [Period reconciliation](../upstream-privacy-period-reconciliation/SPEC.md)
- [Credits billing](../upstream-credits-billing/SPEC.md)
