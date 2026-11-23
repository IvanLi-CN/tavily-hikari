---
status: accepted
supersedes:
  - 0002-scoped-sqlite-and-remote-admission
  - 0005-fair-sqlite-maintenance-admission
  - 0010-observability-sqlite-wal
---

# Move persistent storage from SQLite to PostgreSQL

服务端以 SQLite 承载业务真值与派生观测数据，并发、维护与恢复合同都是围绕单写者和文件 WAL 建立的。为使存储合同
与实现复杂度脱钩，持久化后端一次性切换到 PostgreSQL 18，并只保留一个后端。

## Consequences

- 存储层只支持 PG；`sqlx` 的 SQLite 特性与 `libsqlite3-sys` 移除，不保留兼容后端或特性开关。
- 一个 PG 数据库内保留 `core` 与 `observability` 两个 schema，维持业务/派生边界；不新增跨 schema 原子事务依赖。
- 以下 SQLite 合同退役：单写者串行化、cooperative read budget、写锁竞争退避、按 schema 的 WAL checkpoint、
  `VACUUM`/compaction、页与空闲页指标、`sqlite_master`/`PRAGMA` 探测、HA 触发器与 outbox。
- 依赖单写者串行的路径必须改写为显式事务、行锁或 advisory lock；SQLite 触发器承接的数据规范化必须有等价实现。
- 只保留 `HA_MODE=single`；多节点同步与接管能力退役。
- 迁移工具是独立镜像，与服务端共用同一份 schema 定义，只做 SQLite → PG 单向。类型映射只做亲和性等价，不做
  `timestamptz`/`boolean`/`jsonb` 升级。
- 一致性放行口径为行数、主键集合与列聚合；逐行全列比对不进门槛，残余盲区由列映射单一来源与账本人工复核缓解。
- 回退手段是旧镜像加未被覆盖的旧数据，不是新代码的能力。
- 合同细节见 [sqlite-to-postgresql](../specs/sqlite-to-postgresql/SPEC.md)。
