# SQLite / PostgreSQL 差分回放夹具

同一份二进制、同一份确定性流量，分别跑 SQLite 后端与 PostgreSQL 后端，导出两边的事实表并比较。目标是发现
“同一请求在两种存储后端下写出了不同事实”的迁移回归。

夹具只写本地测试库与临时 SQLite 文件；上游固定为仓库内的桩服务，不连接生产 Tavily。

## 前置条件

- `--binary` 必须支持用 `PROXY_PG_BACKEND=1` 让请求路径真正走 PostgreSQL。夹具不依赖启动日志措辞：它会在
  PostgreSQL 后端上创建一个 token，再用 `psql` 回查 `core.auth_tokens`；查不到就把这次运行判为环境错误并
  以 `2` 退出，不会把“PG 库为空”误报成“两边一致”。
- 手工配额同步（`POST /api/keys/{id}/sync-usage`）要求键创建与配额同步落在同一个后端。迁移未完成时这是
  跨后端的，会以 SQLite 外键错误失败；此时用 `--skip-manual-quota-sync` 跳过该探针，键池余额事实会被标记
  为“未断言”，其余事实照常比较。cutover 完成后不要带这个开关。
- 需要 `psql` 客户端；PostgreSQL 事实通过 `psql -c` 导出，不引入 Python 驱动依赖。
- `--pg-url` 必须指向专用测试库。默认会先删除 `core`、`observability` schema 以及 `public._sqlx_migrations`
  迁移台账，让服务端重新迁移出全新结构；不要指向生产库。
- 夹具会拒绝解析到公网地址的 PostgreSQL 主机；确实需要远端专用测试库时显式传 `--allow-non-local-pg`。
- 桩上游复用 `tests/performance_recovery/mock_upstream.py`，由夹具自行启动。

## 用法

```sh
python3 scripts/pg-parity/run.py \
  --binary target/debug/tavily-hikari \
  --pg-url postgres://parity@127.0.0.1:5432/parity \
  --duration-secs 120 \
  --keep
```

快速回归只跑 PG 并检查其内部不变式：

```sh
python3 scripts/pg-parity/run.py \
  --binary target/debug/tavily-hikari \
  --pg-url postgres://parity@127.0.0.1:5432/parity \
  --duration-secs 30 \
  --skip-sqlite
```

默认端口都是高位且可覆盖：桩上游 `19001`，PG 实例 `58187`，SQLite 实例 `58188`。对应参数为
`--mock-upstream-port`、`--backend-port`、`--sqlite-backend-port`。

## 流程

1. 启动桩上游。
2. 重置 PG 测试库 schema 与迁移台账。
3. 以 `PROXY_PG_BACKEND=1`、`--database-url`、`--dev-open-admin` 启动 PG 实例，等待 `/health`。
4. 手工同步一次 key 配额（`POST /api/keys/:id/sync-usage`），固定余额事实，不依赖后台调度时机。
5. 跑确定性流量。
6. 轮询 `/api/logs` 的 `total`，直到达到本次代理请求数；rebalance 控制面行由 1 秒防抖的后台 writer
   写入，必须等它排空，否则两次运行会导出不同的 `request_logs`。超时（`--persist-timeout-secs`，默认 30 秒）
   按环境错误退出。
7. 等待 rollup flush（`--settle-secs`，默认 5 秒）。
8. 停止实例后再导出事实：停止后不再有写入，SQLite WAL 也不再与读连接竞争，避免半提交快照。
9. 对 SQLite 实例重复 3–8，数据库位于 `<work>/sqlite/core.db`，观测 sidecar 为
   `<work>/sqlite/core-observability.db`。
10. 比较流量记录与事实表，输出报告。

产物都在 `--work-dir`（未指定时为新建临时目录）：`traffic-{postgres,sqlite}.json`、
`facts-{postgres,sqlite}.json`、`report.json`、两个后端与桩上游的日志。成功时默认删除临时目录，失败或
`--keep` 时保留并打印路径。

## 流量

`traffic.py` 只用 seed 与 `--duration-secs` 生成计划：请求顺序、payload、并发分组、周期数都在发第一个请求前
确定；`seq` 在计划阶段分配，所以导出顺序不受线程完成顺序影响。默认 seed 为 `20261123`。

覆盖场景：

- MCP `initialize` / `tools/list` / `tools/call`。
- `POST /api/tavily/search`。
- dashboard 读：`/api/dashboard/overview`、`/api/logs`、`/api/users/rankings`。
- 周期性 `POST /api/tokens`。

MCP 走本地 rebalance facade：它负责 `initialize` 与 `tools/list`，`tools/call` 经 `--usage-base /search`
打到桩上游。这是现有桩能完整支撑的唯一 MCP 形态；透传模式要求上游签发 `mcp-session-id`，现有桩不提供。

记录字段为 `(seq, route, method, rpc_method?, status)`。admin 读接口不一定写入持久事实，比较口径以导出事实
为准，流量记录只负责证明两次运行的请求序列与状态码一致。

`tools/call` 与 `/api/tavily/search` 走同步请求日志；`initialize` / `tools/list` 等本地 MCP 控制面行走防抖
writer。夹具在停止实例前用 `/api/logs` 总数做落盘屏障，避免把异步写入的时序差误判成存储差异。

## 事实口径

两边导出同一个 JSON 结构：

- `auth_token_logs`：总数、按 `result_status` 分组、按 `request_kind_key` 分组。
- `request_logs`：总数、按 `visibility` 分组。
- `api_key_usage_buckets`：按 `day_key` 汇总 `total_requests` 与 `local_estimated_credits`。
- `account_balance`：`api_keys.quota_remaining` 合计。
- `alerts`：总数、按 `group_key` 与 `status` 分组。
- `invariant`：bucket 的 `total_requests` 合计与同一时间窗内 `request_logs`（`visibility='visible'` 且
  `api_key_id IS NOT NULL`）条数比较。

本仓库 schema 与最初任务措辞的差异，按实际列名落地：

- `api_key_usage_buckets` 两边都没有 credits 列；`local_estimated_credits` 存在
  `dashboard_request_rollup_buckets`，因此按同一 `day_key` 从该表汇总。
- 没有 `accounts.balance` 关系；账户余额取当前服务实际维护的 key 池余额
  `api_keys.quota_remaining`（不含已删除 key）。
- 告警事件的分组身份在 `payload_json` 内：`group_key = payload_json.alert_type`，`status` 优先取
  `payload_json.result_status`，其次 `payload_json.job_status`。
- `day_key` 取 `bucket_start` 的 UTC 日期；bucket 本身仍按服务端本地日界生成。这样标签不依赖执行导出时的
  时区或数据库会话时区。
- SQLite 观测表在 attached sidecar；PG 用 `core.` / `observability.` schema 限定。

## 比较与容差

必须完全一致：流量状态码序列、各分组计数、求和、不变式的 bucket 总数与明细条数。

允许差异只记入报告的 `facts.tolerated`，不判失败：

- 自增 id 与其他 `*_id` 字段。
- 时间戳字段（`*_at`、`created_at`、`updated_at`、导出时间）。
- 浮点最后一位（相对误差 `1e-9` 内）。

当前事实集不含 id 与时间戳字段；容差规则仍保留，供后续扩展事实时使用。

## 退出码

| 退出码 | 含义                                                                            |
| ------ | ------------------------------------------------------------------------------- |
| `0`    | 两边一致，且各自内部不变式成立                                                  |
| `1`    | 存在实质差异：流量、分组计数、求和或不变式不一致                                |
| `2`    | 环境错误：二进制或 `psql` 缺失、端口占用、服务起不来、PG 仍是启动门禁、导出失败 |

`--skip-sqlite` 模式下只检查 PG 自身的内部不变式（分组和等于总数、bucket 总数等于可见明细数），不产生
跨后端比较。

## 限制

真实双后端回放待 VM 执行：当前快照的二进制未实现 `PROXY_PG_BACKEND` 的请求路径切换，本地只验证了
SQLite 流量、SQLite 事实导出、PG 事实 SQL、确定性比较与门禁识别。夹具不 push、不建 PR，也不替代
`tests/performance_recovery` 的长时间恢复验证。

## 何时可以跑这个门禁

这是 **cutover 之后** 的门禁，不是迁移中间态的工具。判定标准是行为而非配置：夹具写一个 token，再用
`psql` 回查 `core.auth_tokens`；同时在 PostgreSQL 阶段的 SQLite sidecar 里统计 `request_logs`。只要还有
记账行落在 sidecar，说明存储仍是"半个 port"——此时配额与计费在两个后端各写一半，PostgreSQL 侧会**少执行**
配额，比较结果没有意义。夹具在这种情况下直接以 `2` 退出并说明原因，不会给出"看起来通过"的报告。

证据示例（部分 port，该次运行）：

- PostgreSQL 后端：`/api/tavily/search` 180/180 返回 200
- SQLite 后端：100 次 200 后返回 `429 {"error":"quota_exhausted"}`
- 原因：token 配额与计费明细仍在 SQLite lane 写入，PostgreSQL 侧读不到，于是不执行配额

另有两个真实缺陷是在这个过程中被找出来的，已修复：`resolve_token_log_request_kind_pg` 与
`resolve_request_log_diagnostic_metadata_pg` 从 SQLite 版本复制时保留了 `?` 占位符，PostgreSQL 报
`syntax error at or near "LIMIT"`，导致待计费记录写不进去。
