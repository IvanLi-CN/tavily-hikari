# SQLite 到 PostgreSQL 存储迁移 演进历史

## 变更记录（Change log）

- 2026-11-22: 建立主题，锁定需求与边界；进入 D2（登记与结构）。
- 2026-11-22: 决策记录如下（含被否决的备选）。
- 2026-11-22: 锁定 PG 结构基线 `src/store/pg/migrations/0001_baseline.sql`。基线由当前二进制创建的全新
  SQLite 库导出后映射得到（不是历史 CREATE/ALTER 序列），已在 PostgreSQL 18 上以 `ON_ERROR_STOP` 完整加载：
  85 张 core 表、38 张 observability 表、1092 列、39 个外键、144 个显式索引，列名与 SQLite 源逐表一致。
  9 个触发器不进入结构基线，移植要求与需要替代的 SQLite 函数清单写在基线文件末尾。

## 已决策项（Decisions）

### 迁移执行方式：单窗口全量

选择：一次切换窗口内完成冻结、快照、全量装载、校验与启动。

否决"先迁移核心库上线、观测库历史后补"：`request_logs` 没有 `updated_at`，且插入后老行仍被
`UPDATE`（body 清理、`request_kind`/`visibility`/`api_key_id`/`request_user_id` 回填、计费回填），
无法用时间戳界定增量；观测侧派生表（daily seal、rollup、告警投影）只有单日重算入口，全仓没有"解封重算"
能力，历史后补会污染已 seal 口径。

否决"预导入 + 差分补尾"：需要新增分块哈希与三类差异回放的差分引擎，属于独立工程量，且差分必须落在账本上。

### 校验口径：聚合

选择：行数 + 主键集合 + 列聚合，不一致阻断切换；逐行全列比对不作为放行门槛。

理由：窗口受限时逐行比对会把停机时间推到不可接受；列映射与 COPY 列序由同一份 schema 定义生成，可压住最常见的
行级错位来源。残余风险见 SPEC 的 Accepted Risks。

### 类型映射：只做亲和性等价

选择：`INTEGER`→`bigint`/`integer`（含时间列）、`REAL`→`double precision`、`TEXT`→`text`、`BLOB`→`bytea`。

否决时间列升级为 `timestamptz`、布尔列升级为 `boolean`：既有日界与时区合同建立在整数语义上，升级会让已验证的
合同重新变成未验证状态，并把改写面扩大到全部时间查询。

### 逻辑结构：同一数据库两个 schema

选择：`core` 与 `observability` 两个 schema，同一 PG 数据库。

否决合并为 `public`：会丢失业务/派生边界。否决拆成两个数据库：现有"同事务写业务与派生"的路径（请求日志回收、
统计 flush、待结算计费）会失效。

### 跨 schema 原子性：不新增依赖

选择：继续不承诺跨 schema 原子时刻。虽然 PG 具备该能力，但新增依赖会改变既有并发假设，属于行为变化而非迁移。

### HA：只保留单节点

选择：只保留 `HA_MODE=single` 语义，多节点同步、outbox、接管与相关维护入口退役。移植 HA 到 PG 会显著扩大
工期与风险，且不属于本次迁移目标。

### schema 与迁移机制

选择：DDL 嵌入 `src/store/pg/migrations/`，服务启动自动执行，提供 `--no-auto-migrate`。

否决新建顶层 `migrations/`：构建上下文白名单是安全面，不为迁移放宽。

### 镜像与发布

选择：两个镜像（服务端、迁移工具）同 release、同 version、multi-arch；服务端镜像零 SQLite 依赖；迁移工具二进制
不嵌版本（保持 ADR 0007 的"仅两层随版本变化"不变式）；发布冒烟改为 PG 断言。

### 文档边界

选择：入库文档只描述合同与验收；生产环境事实（主机、域名、容器、挂载点、凭据、实测容量）不入库，演练报告存放于
不入库的位置。

## 待办

- D2 登记表与 PG schema 评审。
- D1 迁移工具、D3 服务端实现、D4 发布流水线、D5 演练。

## 演练发现（2026-11-24）

在生产规模数据上进行首次全量演练，确认以下两点：

- 在线快照用 `VACUUM INTO`，不用 `.backup`。持续写入下 `sqlite3 .backup` 会因源页变化反复重启，
  13 分钟只写出约 2 GB；`VACUUM INTO` 走一次一致性读事务，9.14 GB 库 2 分 20 秒完成，
  17.5 GB 观测库 51 秒完成（压缩后 2.98 GB）。
- `load` 必须挂起用户触发器。快照同时包含派生表与其来源表，而 9 个 PG 触发器会从来源行重建派生行；
  保持触发器启用会在 `core.upstream_reconciliation_work` 上撞主键并中止 COPY。载入事务内
  `DISABLE TRIGGER USER` → COPY → `ENABLE TRIGGER USER`，失败回滚即恢复启用状态。
