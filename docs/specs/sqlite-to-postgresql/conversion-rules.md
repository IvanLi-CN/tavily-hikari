# SQLite → PostgreSQL 转换规则（W2 共用）

本文件是 W2 并行改写的唯一口径。任何与本文冲突的写法都视为错误，不要各自发明。

## 0. 前提

- 目标结构是 `src/store/pg/migrations/0001_baseline.sql`（冻结）。`core` 与 `observability` 两个 schema，列名与 SQLite 完全一致。
- 类型不做语义升级：时间列是 `bigint` 秒，`REAL` 是 `double precision`，`BLOB` 是 `bytea`，布尔值存 `bigint`。
- 本波次接受"编译破窗"：你不要求全仓可编译，但必须保证**你文件内的语法与语义正确**，并把无法在本文件内判定的事项写进报告。

## 1. 占位符

- SQLite `?` → PG `$1..$n`，**每个语句独立编号**，从 1 开始，按出现顺序。
- 多参数语句禁止依赖位置猜测：编号必须与 `bind()` 顺序一一对应。
- 复用同一值的语句重复绑定即可，不要引入命名参数。

## 2. schema 限定

- 业务表加 `core.` 前缀，观测表加 `observability.` 前缀；`ATTACH DATABASE` 全删。
- 禁止依赖 `search_path`。临时表用 `pg_temp.`。
- 视图/CTE 内的表名同样限定。

## 3. 方言替换

| SQLite                                                         | PostgreSQL                                                                                                             |
| -------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `INSERT OR REPLACE INTO t ...`                                 | `INSERT INTO t ... ON CONFLICT (pk) DO UPDATE SET col = excluded.col, ...`（显式列出除主键外所有列）                   |
| `INSERT OR IGNORE INTO t ...`                                  | `INSERT INTO t ... ON CONFLICT DO NOTHING`                                                                             |
| `ON CONFLICT(a,b)`                                             | `ON CONFLICT (a, b)`，被引用的列必须有唯一约束或主键，否则先提出来                                                     |
| `last_insert_rowid()`                                          | `INSERT ... RETURNING id` 并读取返回值；禁止再查 `MAX(id)`                                                             |
| `AUTOINCREMENT` 语义                                           | identity 列，显式插入历史 id 后由 `setval` 对齐（迁移工具负责）；新代码不要手写 id                                     |
| `strftime('%s','now')`                                         | `EXTRACT(EPOCH FROM now())::bigint`                                                                                    |
| `strftime('%s', col)`                                          | `EXTRACT(EPOCH FROM to_timestamp(col))::bigint`（仅当 `col` 是文本时间时使用；整数列直接比较）                         |
| `julianday(x) - julianday(y)`                                  | 直接把 epoch 秒相减                                                                                                    |
| `datetime(x,'unixepoch','localtime')`、`date(...,'localtime')` | 用与服务器时区合同一致的本地日界函数；本地日界计算必须在 Rust 侧完成或使用 `AT TIME ZONE`，禁止用 `localtime` 语义猜测 |
| `json_valid(x)`                                                | `pg_input_is_valid(x, 'jsonb')`                                                                                        |
| `json_type(x)`                                                 | `jsonb_typeof(x::jsonb)`                                                                                               |
| `json_extract(x,'$.a.b')`                                      | `(x::jsonb #>> '{a,b}')`，需要类型时用 `#>`                                                                            |
| `IFNULL(a,b)`                                                  | `COALESCE(a,b)`                                                                                                        |
| 标量 `MIN(a,b)` / `MAX(a,b)`                                   | `LEAST(a,b)` / `GREATEST(a,b)`                                                                                         |
| `GLOB`                                                         | `SIMILAR TO` 或 `~`，按原语义改写并写测试                                                                              |
| `COLLATE NOCASE`                                               | `lower(col)` 比较；需要唯一性时用唯一索引表达式                                                                        |
| `PRAGMA table_info(t)`                                         | `information_schema.columns` / `pg_attribute`                                                                          |
| `PRAGMA foreign_keys`                                          | 删除（PG 始终启用）                                                                                                    |
| `PRAGMA journal_mode/wal_checkpoint/page_count/...`            | 删除，相关代码与指标一并移除                                                                                           |
| `sqlite_master` / `sqlite_schema`                              | `pg_class` + `pg_namespace`                                                                                            |
| `dbstat`                                                       | 删除                                                                                                                   |
| `VACUUM` / `freelist_count`                                    | 删除，改由 autovacuum 承担                                                                                             |
| `LIKE` 大小写                                                  | SQLite `LIKE` 对 ASCII 不区分大小写，PG 区分；需要不区分时用 `ILIKE`                                                   |

## 4. 并发与事务

- `BEGIN IMMEDIATE` → `BEGIN` + 需要的行锁（`SELECT ... FOR UPDATE`）；跨行不变式用 `pg_advisory_xact_lock(key)`，key 由稳定字符串哈希得出并集中登记。
- 原本依赖"单写者串行"的临界区，必须在注释里写明它现在靠什么保证互斥。
- 禁止用 `SERIALIZABLE` 兜底全部事务：只在有明确写偏斜风险的小范围使用，并注明理由。
- 重试：`40001`/`40P01` 可重试，其余不重试；重试次数与退避沿用既有预算，不要新发明。

## 5. 连接与 API 形态

- `SqlitePool` → `sqlx::PgPool`；`sqlx::query()` 保持不引入编译期宏（`query!` 不使用，避免需要数据库在线编译）。
- 动态表名/列名拼接只允许来自内部常量白名单，禁止来自请求输入（既有代码已如此，保持）。
- 时间列读写保持 `i64`；不要改成 `chrono` 类型。

## 6. 禁止事项

- 不得修改 `0001_baseline.sql`；需要新索引/约束/触发器时新增 `0002_*.sql` 并在报告中列出。
- 不得保留 SQLite 分支、特性开关或 `cfg` 兜底。
- 不得改动不属于你切片的文件；共享文件（`src/store/mod.rs`、`src/store/key_store_bootstrap*.rs`、`src/store/sqlite_runtime*.rs`）由协调者所有。
- 不得在代码或文档里写入任何环境/凭据/主机信息。

## 7. 交付报告格式

每个 worker 报告必须包含：

1. 改动文件清单与每文件替换语义类别（占位符 / 冲突 / JSON / 时间 / 锁 / 其他）。
2. 你**无法**在本文件范围判定的项（例如需要新索引、需要跨模块不变式、需要改公共 API）。
3. 你刻意保留的 SQLite 语义差异（如果有）。
4. 你建议的集成测试用例（能证明语义没变）。
