# SQLite 到 PostgreSQL 存储迁移 实现计划

## 交付物

| 编号 | 交付物                                              | 验收                                                                           |
| ---- | --------------------------------------------------- | ------------------------------------------------------------------------------ |
| D1   | 迁移工具镜像（`snapshot`/`load`/`verify`/`report`） | 生产规模快照演练全绿；重复执行结果一致；报告含快照标识、工具版本、逐表三类比对 |
| D2   | SQLite 调用登记表 + PG schema + 迁移方案            | 登记到文件/函数级且无遗漏；DDL 通过评审；退役清单与 ADR 0012 一致              |
| D3   | 服务端 PG-only 实现                                 | clippy 零告警；PG 实例上 `/health`、`/api/summary`、`/mcp`、SPA 关键路由通过   |
| D4   | 发布流水线双镜像 + PG 冒烟                          | 同一 release 产出两镜像 manifest；冒烟断言落到 PG 账本；构建上下文白名单未放宽 |
| D5   | 演练取证                                            | 停机窗口各部分耗时、校验结论、失败处置；报告不入库                             |

## 工作分解

### D1 迁移工具

1. CLI 骨架：`snapshot`、`load`、`verify`、`report`；共享 schema 定义编译期嵌入。
2. `snapshot`：SQLite backup API，校验已提交 WAL 帧进入副本，输出快照标识。
3. `load`：目标非空断言、按表分期装载、索引后建、序列对齐、失败即停并报告表与主键。
4. `verify`：行数、主键集合、列聚合三类比对；输出 JSON 报告。
5. 类型映射：亲和性等价映射表与严格失败路径。

### D2 登记与结构

1. 逐模块登记 SQLite 专属调用：运行时、cooperative 读预算、contention 日志、WAL checkpoint、VACUUM/compaction、
   页与空闲页指标、`sqlite_master`/`PRAGMA` 探测、`ATTACH`、`BEGIN IMMEDIATE`、SQLite 触发器、HA 触发器与 outbox。
2. `core` 与 `observability` 两份 schema 的定义、约束、索引与序列策略。
   状态：已锁定 `src/store/pg/migrations/0001_baseline.sql`，并在 PostgreSQL 18 上完成加载验证。
3. 触发器与规范化逻辑的承接方案清单。
4. 迁移方案文档：快照边界、装载顺序、窗口内步骤、校验点、失败处置。

### D3 服务端实现

1. `sqlx` 依赖与驱动切换；连接池与事务封装。
2. 存储层按模块改写：键与令牌、配额与账本、会话与认证、请求日志与统计、任务与维护、告警投影。
3. 删除被退役模块与维护二进制；补齐 PG 版维护入口。
4. 测试改造：PG 基线、CI 服务容器、并发回归与确定性时间合同保持。

#### 现状（2026-11-27）

含 SQL 的 `src/store` 模块共 78 个，其中 38 个已有 PG 路径（`self.pg()` 分支或 `*_pg.rs`
双胞胎模块）。剩余 30 个分三类：

| 类别         | 模块                                                                                                                                                                                                                                                                                    | 处置             |
| ------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------- |
| 待移植（0）  | 无：`key_store_alert_canonical_groups` 已在本轮完成（`rowid` → `ctid`、`INDEXED BY` 去掉、`build_source_rowid_upper_bound` → `projection_revision` 栅栏）                                                                                                                               | —                |
| 待退役（4）  | `key_store_ha`、`key_store_ha_gc_online`、`key_store_ha_schema`、`key_store_ha_watermarks`                                                                                                                                                                                              | 删除（ADR 0004） |
| 待删除（14） | `sqlite_runtime`、`sqlite_runtime_cooperative`、`immediate_transaction`、`key_store_observability_sidecar`、`key_store_bootstrap{,_legacy}`、`key_store_migrations_{a,b}`、`key_store_schema_migrations{,_convergence}`、`reconciliation_projection_controller`、`mod` 中的 SQLite 装配 | W3-cleanup       |

本轮完成的模块：`key_store_alert_projection`（投影族整体）、`key_store_admin_privacy_read`、
`key_store_alert_canonical_catalog`、`key_store_alert_canonical_semantic_reduction`、
`key_store_alert_canonical_groups`。
`key_store_alerts`、`key_store_reconciliation_{progress,key_observations}`、
`key_store_upstream_reconciliation_{admission,research}`、`key_store_quota_schema_semantic_migration`、
`key_store_admin_passkey_schema` 已在更早批次接入 PG。

告警与对账读模块都持有 "一个响应一个只读快照" 的合同，移植时先落 PG 只读快照（
`REPEATABLE READ READ ONLY`），再逐函数接 `self.pg()`。

已完成入口（均带 `pg-write-paths` 跨后端对拍测试）：

- `PgReadSession`：`REPEATABLE READ, READ ONLY` 单事务，替代 SQLite 读会话的一致性职责；
  不带 cooperative 预算（那是为了管不可取消的原生 SQLite 工作）。
- 告警事件页：`fetch_admin_alert_events_page_for_canonical_snapshot`、
  `fetch_admin_alert_events_page_for_operation`（复用双方言 CTE builder）。
- 对账进度：`daily_reconciliation_progress`（含空窗口 PG 冒烟测试）。
- 告警分组 CTE：`instr`/`strftime` 本地日月中点/`printf` 窗口键改为按方言生成；
  `UNION ALL` 两支在 PG 下使用显式类型字面量（SQLite 动态类型容忍、PG 按位置严格匹配）。
  分组 SQL 已在 `pg-write-paths` 中直接执行验证。
- 对账 key observations：读与写都走 PG（写侧用 `FOR UPDATE` 锁工作行代替 SQLite 立即事务）。
- 对账运行统计与 research 不可用标记：`upstream_reconciliation_last_run_stats`、
  `mark_upstream_reconciliation_research_unavailable` 已接 PG，并有跨库/落库断言。
- 对账 research 候选选择：`next_upstream_reconciliation_research_candidates` 有 PG twin，
  游标遍历、hydrate 与冷却计数同源，页面装配为双方言共享纯函数。
- 告警分组页：`fetch_admin_alert_groups_page` 的 PG 分支在一个可重复读会话内完成计数、
  分页、每组最新事件与母组补全。
- PG 迁移新增 `0003_singletons.sql`：播种 5 张对账单例表的 `id='local'`，否则 PG 读全部为空。
- 告警投影写入与状态读取同批接 PG：`persist_alert_projection_slice` 与
  `alert_projection_status` 走同一事务模型（切片单事务、状态更新按 `generation` 乐观比较），
  写入侧不再依赖 SQLite 的 owned immediate 包装。

#### PostgreSQL 类型适配清单（每条来自一次真实 CI 失败）

| 现象                                            | 根因                                                | 处置                                                                          |
| ----------------------------------------------- | --------------------------------------------------- | ----------------------------------------------------------------------------- |
| `UNION types text and bigint cannot be matched` | 分支列类型不同                                      | 给整数列/`NULL` 显式定型                                                      |
| `INT8 ... TEXT` on decode                       | `json_extract` → `->>` 恒返回 text                  | 整数成员加 `::bigint`                                                         |
| `invalid input syntax for type bigint: "true"`  | payload 存 JSON 布尔                                | `CASE WHEN ... = 'true' THEN 1 ELSE 0 END`，且结果 `::bigint`（CASE 是 int4） |
| `column "bound_users" does not exist`           | PG 的 `HAVING` 不支持 select 别名                   | `HAVING` 重复完整聚合表达式                                                   |
| `SUM(...)` decode 失败                          | PG 把 bigint 的 `SUM` 提升为 numeric                | `CAST(... AS BIGINT)`                                                         |
| `relation "x" does not exist`                   | 未加 schema 前缀                                    | `core.` / `observability.`                                                    |
| `relation "observability.core.x"`               | 重写把已带前缀的再前缀一次                          | 先替换 `core.<已带前缀>` 回原样                                               |
| 投影切片返回 0 行                               | `AlertProjectionSourceKey::source_id` 应为裸 id     | 只让 `row_sort_id` 带前缀                                                     |
| 投影 CTE 的类型修复无效                         | 它走 `alert_payload_projection_sql`，不经通用重写器 | 在该函数内处理类型                                                            |

**排查顺序**：先确认 SQL 由哪条路径生成（通用重写器 / 专用 helper），再核对该路径的类型处理；否则会陷入「同一错误反复修复」。

#### 迁移硬约束（由真实失败得出）

1. 纯装配逻辑可抽成双方言共享函数，但 SQLite 的 `complete_query_or_defer` /
   `complete_native_read` 必须留在原分支——它们负责归还读连接；抽走会让连接被丢弃。
2. 方言差异必须逐条核对：无默认值的 NOT NULL 列、`CASE ... THEN 1 ELSE 0`（PG 为 int4）、
   `SUM` 与窗口 `SUM`（PG 为 numeric）、`UNION ALL` 分支按位置严格匹配。
3. **跨表事务单元必须整库迁移**。告警投影族（切片推进器 + `persist_alert_projection_slice`
   - `alert_projection_status` + 投影读路径）是同一个逻辑事务；只迁写入侧会让
     generation 乐观比较永不匹配（实测：`alert projection state changed while a slice was in flight`），并留下未回滚的投影行。

W3 删除顺序也由此确定：投影族整体移植完成前，投影读路径的 PG 化没有意义——PG 的
`observability.dashboard_alert_projection_events` 在生产里仍是空的。

#### W3 可执行删除顺序（实测调用链）

`observability_sidecar` 与 `bootstrap_legacy` 不是被独立的生产路径调用，而是被 SQLite 的
attach 探测（`store/mod.rs` 的 `select_observability_attach_path` →
`configure_observability_journal_mode`）调用。因此删除顺序应当是自顶向下：

1. 让 `KeyStore::open` 在 `pg_pool` 存在时完全跳过 SQLite attach 探测（当前只是绕过读取，初始化仍在跑）。
2. 删除该探测及其 `configure_observability_journal_mode` 调用 → `observability_sidecar` 失去调用者。
3. 删除 `observability_sidecar` → `bootstrap_legacy` 失去调用者。
4. 删除 `bootstrap_legacy`。
5. `reconciliation_projection_controller` 的类型 `ReconciliationProjectionSliceOutcome` 被
   `key_store_upstream_reconciliation.rs` 与 `proxy_quota_sync_and_jobs.rs` 引用，需先把该类型
   移到独立模块或改为新投影路径的类型，再删旧实现。
6. 上述完成后，`Cargo.toml` 的 `sqlx/sqlite` 与 `libsqlite3-sys` 才可移除；此时编译器会列出
   剩余全部 SQLite 引用（当前 173 个文件），逐条删除或改为 PG。

每个步骤都要保持 `cargo check` 通过，且不能在同一提交里既删调用者又留着被调用者。

#### W3 阻塞项（复核后修正）

- `key_store_ha*`、`key_store_ha_gc_online` 仍被 `server/schedulers.rs` 与
  `tavily_proxy/proxy_ha.rs` 调用，不能仅因 ADR 0004 就删除。
- `key_store_observability_sidecar` 仍被 `mod.rs` 与 `bin/support/sqlite_sidecar.rs` 使用。
- `reconciliation_projection_controller` 仍被 `key_store_upstream_reconciliation.rs` 的
  `advance_slice` 调用。
- 因此 W3 只能在「投影族整体迁移」与「HA/sidecar 替代路径就位」之后开始。

尚未完成：告警分组页与目录的 PG 读取路径（SQL 已可执行，差行解码与两个 session 化辅助函数）、
告警投影写入侧、`admin_privacy_read`、对账 research/admission。

告警投影的“状态读取”与“写入”必须同批落地：写入仍走 SQLite 时，PG 读到的覆盖状态永远停在
`projecting`，会让 `alert_projection_rebuild_is_idempotent` 这类收敛断言无法完成。

### D4 发布

1. 第二个镜像进入 release 流水线：digest 分集、manifest 合并、平台校验、标签与版本标注。
2. 冒烟改源：把直读 SQLite 文件的断言改为 PG 查询；保留计费链路覆盖。
3. 版本嵌入遵守 ADR 0007：迁移工具二进制不嵌版本，版本只出现在镜像标签与报告字段。

### D5 演练

1. 在生产规模快照上跑完整流程，记录各阶段耗时。
2. 复核账本类表结论；记录失败与重试处置。
3. 报告存放位置不入库；只在 SPEC/IMPLEMENTATION 保留不含生产信息的摘要。

#### 首轮演练（2026-11-24，生产数据副本）

全链路在隔离 VM 内完成，生产实例只读。各阶段耗时与结论：

| 阶段                           | 规模                    | 耗时          |
| ------------------------------ | ----------------------- | ------------- |
| 在线快照（两库 `VACUUM INTO`） | 9.14 GiB + 17.5 GiB     | 2m20s + 51s   |
| 传输（服务器 → 中转 → VM）     | 11.84 GB                | 2m30s + 3m20s |
| `s2pg snapshot`                | 123 表 / 23,416,266 行  | 3m22s         |
| `s2pg load --reset`            | 23,416,266 行 / 17 序列 | 10m33s        |
| `s2pg verify`                  | 123 表三类比对          | 约 6m         |

结论：`overall: OK`，两侧行数均为 23,416,266，失败项 0。两段 sha256 与源文件一致。

演练首次暴露并修复的三个工具缺陷见 [HISTORY.md](HISTORY.md)：`.backup` 在持续写入下反复重启，
改用 `VACUUM INTO`；`load` 必须挂起用户触发器；`verify` 改按列集合而非列顺序比对。

报告与快照保存在不入库的位置，不随仓库分发。

## 顺序与停靠点

1. D2 先行（登记与结构决定工作量和类型映射）。
2. D1 与 D3 可并行，但 D1 的 `load` 依赖 D2 的 DDL。
3. D4 在 D3 可发布后进行。
4. D5 在 D1、D3、D4 完成后执行；切换执行需另行授权。

## 未决与后续

- 无。本计划的开放问题已在 [HISTORY.md](HISTORY.md) 记录为已决策项。
