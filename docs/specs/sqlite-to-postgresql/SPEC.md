---
status: active
---

# SQLite 到 PostgreSQL 存储迁移

## Context and Scope

服务端当前以 SQLite 作为唯一持久化后端：一个核心库文件承载业务真值，另一个独立文件被 `ATTACH` 为 `observability`
schema 承载派生观测数据。两者存在同事务写入路径（请求日志回收、统计 flush、待结算计费）。

本主题把持久化后端一次性切换为 PostgreSQL 18：服务端只支持 PG，另交付一个离线迁移工具镜像，负责把既有 SQLite
数据搬到 PG 并输出可复现的一致性报告。两个镜像都随现有 CI 发布流程发布。

范围仅包含开发与验证。部署拓扑、实例位置、容量规划、凭据与切换执行不属于本主题；本主题的代码与文档也不得反向约束这些决定。

## Terms and Interfaces

- **Core database**：业务真值所在库（键、令牌、配额、账本、会话、任务）。
- **Observability database**：派生观测数据所在库（请求日志、统计桶、完整性台账、告警投影）。
- **Cutover window**：从停止旧服务到新服务通过健康检查的时间段。
- **Migration snapshot**：从 SQLite 导出的一致性副本，必须包含尚未 checkpoint 的已提交帧。
- **Verification report**：迁移工具输出的机器可读一致性证据。
- 术语以 [GLOSSARY.md](../../../GLOSSARY.md) 为准；`observability SQLite sidecar` 说法在本主题后退役。

## Requirements

### REQ-PG-001 唯一持久化后端

服务端 MUST 只支持 PostgreSQL 18。`sqlx` 的 `sqlite` 特性、`libsqlite3-sys`、运行时与构建期 SQLite 依赖 MUST
被移除；不得保留 SQLite 分支、特性开关或"兼容后端"。

### REQ-PG-002 逻辑结构

同一 PG 数据库内 MUST 使用两个 schema：`core` 与 `observability`，分别对应 Core database 与 Observability
database。禁止把两者合并成单一 schema，也禁止拆成两个独立数据库（会破坏现有同事务写入路径）。

### REQ-PG-003 跨 schema 原子性

迁移 MUST NOT 新增对跨 schema 原子事务的依赖。既有合同明确不承诺两个存储的同一原子时刻，迁移后继续保持该合同。

### REQ-PG-004 单节点语义

服务端 MUST 只保留 `HA_MODE=single` 语义。多节点同步、outbox、接管与相关维护入口 MUST 删除或退役，不得以
PG 版本替换形态继续存在。

### REQ-PG-005 迁移工具接口

迁移工具 MUST 提供 `snapshot`、`load`、`verify`、`report` 子命令，输入为 SQLite 数据库文件路径与 PG 连接串，
只做 SQLite → PG 单向。要求：

- 快照 MUST 通过 SQLite backup API 或等价机制取得一致副本；禁止直接拷贝主文件。
- `load` MUST 拒绝写入非空目标（显式重置参数除外），并在写入后把每个自增序列对齐到 `max(id)+1`。
- 快照、装载、校验 MUST 由同一二进制完成，正式切换与演练使用同一份代码路径。
- 失败 MUST 立即停止并报告具体表与主键，不得跳行继续。

### REQ-PG-006 类型映射

类型映射 MUST 只做亲和性等价，不做语义升级：

- `INTEGER` → `bigint`/`integer`（含时间列，保持既有的整数 epoch 语义）
- `REAL` → `double precision`
- `TEXT` → `text`
- `BLOB` → `bytea`

MUST NOT 引入 `boolean`、`jsonb`、`timestamptz` 等语义升级。理由是既有日界、时区与区间比较合同全部建立在整数语义上，升级会把已验证的合同重新变成未验证状态。类型不匹配（例如 `INTEGER` 列出现文本或浮点）MUST 直接失败，禁止静默转换。映射表 MUST 与 DDL 同源，并作为验收产物提供。

### REQ-PG-007 一致性口径

`verify` MUST 逐表比对行数、主键集合与列聚合（计数、数值和/极值、文本长度与校验和分布），并在报告中标明快照标识与工具版本。任一表不一致 MUST 阻断切换。逐行全列比对不属于放行门槛（见 Accepted Risks）。

### REQ-PG-008 Schema 与迁移机制

PG DDL MUST 以版本化迁移文件形式嵌入服务端二进制（`src/store/pg/migrations/`，`sqlx::migrate!`），不得新建顶层
`migrations/` 目录，也不得放宽构建上下文白名单。服务启动 MUST 自动执行未应用的迁移，并提供 `--no-auto-migrate`
供运维预先执行；关闭自动迁移时，缺表 MUST 以明确错误拒绝启动，不得静默降级。

### REQ-PG-009 SQLite 合同退役

以下既有合同 MUST 随实现退役，并在 ADR 0012 中登记：单写者串行化假设、cooperative read budget 与 progress
handler、写锁竞争退避与 contention 日志、按 schema 的 WAL checkpoint、`VACUUM`/compaction 维护、`page_count`/
`freelist_count` 指标、`sqlite_master`/`PRAGMA` 探测、HA 触发器与 outbox。对应代码 MUST 删除，不得留空壳。

### REQ-PG-010 并发语义显式化

凡依赖单写者串行的路径（`BEGIN IMMEDIATE` 及等价写法）MUST 改写为显式事务、行锁或 advisory lock，并逐个登记其替代语义。SQLite 触发器承担的数据规范化 MUST 以 PG 触发器或应用层等价实现承接，禁止直接丢弃。

### REQ-PG-011 维护入口

请求日志回收、统计恢复、观测侧迁移等维护入口 MUST 提供 PG 实现或明确退役。仅服务 SQLite 的维护二进制 MUST
从镜像移除；不提供"PG 兼容替身"。

### REQ-PG-012 测试与 CI 基线

测试 MUST 以 PG 18 为基线：CI 使用服务容器提供 PG，禁止再创建 SQLite 测试库。既有确定性时间、分片执行与
并发回归合同 MUST 保留。

### REQ-PG-013 镜像与发布

发布 MUST 产出两个镜像（服务端、迁移工具），同一 release、同一 version、multi-arch。服务端镜像 MUST 零 SQLite
依赖。发布冒烟 MUST 改为 PG 断言（现有直读 SQLite 文件的冒烟脚本必须换源），不得以最小健康检查替代计费链路覆盖。

### REQ-PG-014 演练取证与文档边界

迁移 MUST 以演练取证：在受控环境用生产规模快照完成一次全流程，产出耗时、校验结论与失败处置记录。演练报告 MUST
放在不会进入远程仓库的位置；入库文档 MUST NOT 出现主机名、域名、容器名、挂载点、凭据或实测容量。

### REQ-PG-015 回退边界

代码层 MUST NOT 保留 SQLite 路径。回退手段为"旧镜像 + 未被覆盖的旧数据"，不是新代码的能力；文档与 runbook MUST
如实表述这一点，不得暗示可自动回切。

## Out of Scope

- 部署形态、实例位置、容量与磁盘布局、凭据管理、端口与网络。
- 增量、分段或双写迁移（含差分补尾引擎）。
- 数据裁剪、归档与保留策略变更。
- 多节点 HA。
- 前端行为与对外 API 契约变更。

## Accepted Risks

- **聚合口径的盲区**：行级错位而聚合不变（例如 COPY 列序错误导致两行互换值）无法被发现。缓解：列映射与 COPY
  列序由同一份 schema 定义生成，单一来源；账本类表在演练报告中单列人工复核结论。
- **单窗口全量装载**：Cutover window 等于冻结 + 快照 + 装载 + 校验，受目标库写入能力制约。缓解：schema 预建、
  索引后建、装载期会话调优、按表并行、校验用聚合而非逐行；窗口长度以演练实测为准，不作估算承诺。
- **无 SQLite 双后端**：上线后代码层无法回到 SQLite，任何功能回归只能向前修复。

## Verification

### VER-PG-001

- covers: `REQ-PG-001`, `REQ-PG-009`
- 证据：`Cargo.lock` 与镜像依赖清单不含 SQLite；被退役模块的文件清单与删除记录。

### VER-PG-002

- covers: `REQ-PG-002`, `REQ-PG-003`, `REQ-PG-008`
- 证据：迁移文件内容、启动自动迁移日志、`--no-auto-migrate` 下缺表拒绝启动的实测输出。

### VER-PG-003

- covers: `REQ-PG-005`, `REQ-PG-006`, `REQ-PG-007`
- 证据：演练的 Verification report（行数、主键集合、列聚合三类逐表结果）与类型映射表。

### VER-PG-004

- covers: `REQ-PG-010`, `REQ-PG-011`
- 证据：`BEGIN IMMEDIATE` 与触发器替代清单，逐项标注新语义与覆盖测试。

### VER-PG-005

- covers: `REQ-PG-012`
- 证据：CI 全量测试通过记录；测试代码中不再出现 SQLite 建库路径。

### VER-PG-006

- covers: `REQ-PG-013`
- 证据：一次 release 同时产出两个镜像的 manifest 与版本标注；PG 冒烟执行日志。

### VER-PG-007

- covers: `REQ-PG-014`, `REQ-PG-015`
- 证据：演练报告（不入库）摘要，与入库文档的生产信息扫描结果。
