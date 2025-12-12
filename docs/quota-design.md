# Tavily Hikari 配额与用量设计

本文记录 Access Token（`/mcp` 访问令牌）相关的用量统计与配额（quota）
实现方式，以及各张表的职责分工。目标是：

- 在**限额判断**上使用轻量聚合数据结构，避免每次请求都扫大表；
- 接受少量误差，换取更低的运行时开销；
- 为未来的 `auth_token_logs` 清理策略留出空间。

## 数据模型与职责

围绕 Access Token 的用量，我们主要依赖四张表：

1. `auth_tokens`
   - 每个 `/mcp` Access Token 一行。
   - 关键字段：
     - `id`：4 位短 ID，对应 `th-<id>-<secret>` 中的 `<id>`；
     - `total_requests`：该 token 自创建以来的总请求次数；
     - `last_used_at`：最后一次使用时间。
   - 角色：管理视图的主表，作为概要统计的最终存储。

2. `auth_token_logs`
   - **按 Token 维度的明细日志表**。
   - 每条 `/mcp` 调用（无论成功或失败）写一行：
     - `token_id`、`method`、`path`、`query`；
     - `http_status`、`mcp_status`（从 Tavily JSON 中解析）；
     - `result_status`：`success` / `error` / `quota_exhausted` / `unknown`；
     - `error_message`（可空）；
     - `created_at`：秒级时间戳。
   - 目前用途：
     - Token 详情页的日志列表、Usage 图表；
     - Token 级汇总统计（成功 / 失败 / quota_exhausted 计数）；
     - 启动时的数据对账（`migrate_data_consistency` 用它重算
       `auth_tokens.total_requests` 与 `last_used_at`）。

3. `token_usage_buckets`
   - **轻量聚合计数表**，只保存最近一段时间的“桶”。
   - 主键：`(token_id, bucket_start, granularity)`；
   - 字段：
     - `granularity = 'minute'`：按分钟聚合，主要用于最近 1 小时窗口的**业务配额**；
     - `granularity = 'hour'`：按小时聚合，主要用于最近 24 小时窗口的**业务配额**；
     - `granularity = 'request_minute'`：按分钟聚合，统计“任意请求”次数，用于**每小时原始请求限频**（见下文）；
     - `count`：该桶内的请求次数。
   - 保留策略：
     - 通过 `delete_old_usage_buckets` 定期删除早于
       `now - BUCKET_RETENTION_SECS` 的数据（当前为 48 小时），
       只保留配额判断需要的近 24 小时上下文。

4. `auth_token_quota`
   - **月度配额计数表**。
   - 每个 token 一行：
     - `month_start`：当前生效的月份起点（UTC 月初）；
     - `month_count`：本月累计使用次数。
   - 行为：
     - 跨月时，通过 UPSERT 逻辑把 `month_start` 推进到新月，
       并自动重置 `month_count` 为 1（即从新月的第一条请求开始重新计数）。

补充：`request_logs` 是按 Tavily API Key 维度的日志，与 Access Token
无直接配额关系，但用于全局概览与按 key 的统计。

## 配额计算路径（按请求）

当客户端携带 `th-xxxx-...` 调用 `/mcp` 或 `/api/tavily/*` 时，后端大致流程如下：

1. 在 `src/server.rs` 中解析出 `token_id`；
2. 先调用 `proxy.check_token_hourly_requests(token_id)` 做**每小时任意请求限频**：
   - 内部逻辑（`TokenRequestLimit::check`）：
     - 计算当前分钟桶：`minute_bucket = now_ts - (now_ts % 60)`；
     - 对该 token 执行：
       - `increment_usage_bucket(..., 'request_minute')`；
       - `sum_usage_buckets(..., 'request_minute', hour_window_start)` → 最近 1 小时“任意请求”用量；
     - 组合成 `TokenHourlyRequestVerdict`，并与
       `TOKEN_HOURLY_REQUEST_LIMIT` / `TOKEN_HOURLY_REQUEST_LIMIT` 环境变量
       覆盖后的值比较（默认 **500 次 / 小时 / token**）。
   - 若 verdict 不允许（`!allowed`），立即返回 `429 Too Many Requests`，并记录一次
     `quota_exhausted` 的尝试日志（错误信息为“hourly request limit reached for this token”），**不会再进入业务配额检查与上游调用**。

3. 对于**有业务成本**的调用，再进入 `proxy.check_token_quota(token_id)` 的配额判断：
   - 内部逻辑（`TokenQuota::check`）仅对“业务调用”生效（见下一节 MCP 非工具调用白名单），对应括号中的注释：
     - 计算当前的分钟与小时桶：
       - `minute_bucket = now_ts - (now_ts % 60)`；
       - `hour_bucket = now_ts - (now_ts % 3600)`。
     - 针对该 token：
       - `increment_usage_bucket(..., 'minute')`； ← 业务配额：小时窗口
       - `increment_usage_bucket(..., 'hour')`； ← 业务配额：日窗口
       - `increment_monthly_quota(..., month_start)`； ← 业务配额：月窗口
     - 再通过：
       - `sum_usage_buckets(..., 'minute', hour_window_start)` → 最近 1 小时**业务用量**；
       - `sum_usage_buckets(..., 'hour', day_window_start)` → 最近 24 小时**业务用量**；
       - `increment_monthly_quota` 的返回值 → 本月**业务用量**；
       - 组合成 `TokenQuotaVerdict`。
   - 若 verdict 不允许（`!allowed`），立即返回 429，并记录一次
     `quota_exhausted` 的尝试日志（错误信息为 “token quota exceeded on ... window ..."）。

4. 若两层限额都允许，则继续调用 Tavily 上游；返回响应后，调用
   `record_token_attempt` 写入：
   - 一条 `auth_token_logs` 明细；
   - 更新 `auth_tokens.total_requests` 与 `last_used_at`。
   - 注意：此处**不会再更新** `token_usage_buckets` 或 `auth_token_quota`，
     聚合计数完全由 `check_token_quota()` 驱动。

### MCP 非工具调用白名单（不计入业务配额）

在 MCP 模式下，Hikari 现在对“业务配额”（`TOKEN_HOURLY_LIMIT` /
`TOKEN_DAILY_LIMIT` / `TOKEN_MONTHLY_LIMIT`）只计入真正触发 Tavily 工具的调用：

- **计入业务配额：**
  - `method = "tools/call"`：工具调用（例如 Tavily Search / Extract / Crawl / Map 等）。
- **不计入业务配额（仍计入“任意请求”限频）：**

  | 类别           | MCP method                       | 说明                           | 匹配方式                                                     |
  | -------------- | -------------------------------- | ------------------------------ | ------------------------------------------------------------ |
  | 工具发现       | `tools/list`                     | 列出可用工具列表，仅做能力发现 | JSON body 中 `method == "tools/list"`                        |
  | 资源列表       | `resources/list`                 | 列出资源清单                   | `method == "resources/list"`                                 |
  | 资源模板列表   | `resources/templates/list`       | 列出资源模板                   | `method == "resources/templates/list"`                       |
  | 资源读取       | `resources/read`                 | 读取 MCP 资源内容              | `method == "resources/read"`                                 |
  | Prompt 列表    | `prompts/list`                   | 列出预设 prompt                | `method == "prompts/list"`                                   |
  | Prompt 获取    | `prompts/get`                    | 获取单个 prompt                | `method == "prompts/get"`                                    |
  | 通知 / 订阅    | `notifications/*` 系列           | 用于资源/工具变更通知          | `method` 以 `notifications/` 前缀开头                        |
  | 其它元数据调用 | 规范中新增的非 `tools/call` 方法 | 例如握手、能力协商等元数据请求 | 默认**按业务调用处理**，只有在确认“无业务成本”后才加入白名单 |

实现上，`src/server.rs` 中的 `mcp_request_counts_toward_business_quota` 逻辑采用“**非业务调用白名单**”：

- 若 `path` 不以 `/mcp` 开头 → 一律视为有业务成本（例如 `/api/tavily/*`，始终计入业务配额）；
- 若 `path` 以 `/mcp` 开头：
  - 解析 JSON body，读取 `method` 字段；
  - 仅当 `method` 落在上表所列的方法集合中时，视为“非业务调用”（不计入业务配额，仅计入每小时任意请求限频）；
  - 对于缺少 `method`、解析失败或任何未在白名单中的新 method，一律视为“业务调用”，正常走 `check_token_quota()` 并消耗 `TOKEN_*_LIMIT`。

这样可以保证：

- MCP 协议层的握手 / 工具发现 / 资源列举等“无上游业务成本”的调用，不再消耗业务配额；
- 真正触发 Tavily 搜索 / 抓取 / 提取等操作的 `tools/call` 调用，仍然严格受小时 / 日 / 月配额限制；
- 同时，所有经鉴权的请求都会受到统一的“每小时任意请求次数”保护，避免恶意刷流量。

### 近似计数与误差来源

由于 `increment_usage_bucket` / `increment_monthly_quota` 发生在配额检查阶段，
而明细日志写入在后续，二者之间不是同一事务，允许出现：

- 配额计数表中已经 +1，但 `auth_token_logs` 还未来得及写入；
- 进程崩溃 / 网络错误导致本次请求只“算在配额里”，但未留明细记录。

从线上数据库拷贝的实际情况看：

- 最近 24 小时窗口内，各 token 的
  `token_usage_buckets`（`granularity='hour'`）累计值与
  `auth_token_logs` 计数差值在 `[-4, +2]` 的小范围内；
- 所有活跃 token 的 24 小时用量远低于 `TOKEN_DAILY_LIMIT = 500`；
- `auth_tokens.total_requests` 与 `auth_token_logs` 全量计数严格一致。

这符合预期的“近似计数”行为：短期窗口内允许少量正负误差，但不会出现
数量级错误或系统性偏差。

对于月度配额（`auth_token_quota.month_count`）：

- 部分 token 的 `month_count` 明显小于当月实际请求数；
- 原因在于：
  - `auth_token_quota` 的填充依赖 `check_token_quota` 路径；
  - 历史数据在引入配额逻辑前不会被回填；
- 默认月度限额 `TOKEN_MONTHLY_LIMIT = 5000`（可通过环境变量 `TOKEN_MONTHLY_LIMIT` 覆盖）较高，当前实际流量远未接近。
- 结论：**当前月度用量是“从启用配额逻辑开始计”的近似值**，
  足以驱动“月度限额是否接近耗尽”的判断，但不适合作为严格审计数。

## UI 用量字段的数据来源

结合现有前后端实现，可以将“界面用量数据”划分为两类：

1. **配额相关（必须轻量、可近似）**
   - Access Token 列表与 Token 详情页中的“Quick Stats”：
     - `quota_state`（normal/hour/day/month）；
     - `quota_hourly_used` / `quota_daily_used` / `quota_monthly_used`；
     - 对应限额与 reset 时间 `*_limit` / `*_reset_at`。
   - 这些字段全部来自：
     - `token_usage_buckets`（minute/hour）；
     - `auth_token_quota`（month）；
     - 以及常量 `TOKEN_*_LIMIT`。
   - 后端路径：
     - `TavilyProxy::populate_token_quota` → `TokenQuota::snapshot_many` →
       `sum_usage_buckets_bulk` / `fetch_monthly_counts` /
       `earliest_usage_bucket_since_bulk`。
   - 结论：**配额 UI 已完全基于轻量聚合表实现，不依赖 `auth_token_logs`。**

2. **审计 / 图表相关（可以用明细表，配合未来保留期）**
   - Token 详情页的 Usage Snapshot、日志表、Usage 图表：
     - `TokenSummary`（period 维度的成功/失败/quota_exhausted 计数）；
     - Token 日志列表（分页）；
     - 按小时 / 自定义秒数聚合的 Usage 直方图。
   - 当前数据来源：
     - `auth_token_logs`（通过时间窗口 + `GROUP BY` 聚合）；
   - 公共页面的 Access Panel / SSE metrics：
     - `SuccessBreakdown` 和 `TokenSuccessBreakdown` 使用的是
       `request_logs`（按 Tavily API Key 维度），对 `auth_token_logs`
       无额外压力。
   - 这些视图主要用于调试与观察，不参与限额决策。

   > 设计约束（后续演进方向）：
   > - 后续会对 `auth_token_logs` 施加**保留期/清理策略**（例如只保留最近 N 天，
     > 或对关闭的访问令牌完全删除历史日志）；
   > - 因此，**界面中除“最近请求记录”列表外，其余用量/统计展示都不应依赖
     > `auth_token_logs`**，而应切换到以下数据源之一：
   > - 近实时窗口（1 小时 / 24 小时）：`token_usage_buckets`（按 minute/hour 聚合）；
   > - 月度用量：`auth_token_quota`；
   > - Tavily API Key 维度的整体成功/失败统计：`request_logs`；
   > - 如需“按 token + 状态”的长期 Usage 图表，可新增聚合表（例如
     > `token_usage_stats`），周期性从 `request_logs` 或上游 Usage API 汇总写入。
     > 换句话说：`auth_token_logs` 的长期定位是“短期可观测性日志 + 最近请求列表
     > 数据源”，而不是所有用量指标的唯一真相源。只要聚合表设计得当，即使截断
     > 历史 `auth_token_logs`，配额和界面上的用量展示都能保持语义合理。

## 业务成本口径用量

为解决 Token Leaderboard / Token Detail 中 `today_total`、`month_total`、`all_total`
用量虚高的问题，需要让 UI 的“今日/本月/总量”统计与业务配额口径一致。

### 现状问题

- 业务配额（小时/日/月）已经通过 `mcp_request_counts_toward_business_quota` 对
  `/mcp` 的非业务方法（`tools/list`、`resources/*`、`prompts/*`、`notifications/*`）
  做了白名单剔除；
- 但 `token_usage_stats` 的 rollup 过去聚合 `auth_token_logs` 全量记录，
  仍包含上述无成本调用，导致 UI 的今日/本月/总量虚高；
- 两套口径不一致，会出现“配额不涨但 UI 用量涨”的偏差。

### 实现要点

1. **在明细日志中落库 billable 标记**
   - 为 `auth_token_logs` 新增列 `counts_business_quota INTEGER NOT NULL DEFAULT 1`，
     表示该条日志是否计入业务成本用量。
2. **写日志时传入真实口径**
   - `record_token_attempt` / `insert_token_log` 增加参数 `counts_business_quota: bool`。
   - `/mcp` 路径：复用 `mcp_request_counts_toward_business_quota(path, body)` 的判定结果；
   - `/api/tavily/*` 路径：一律视为有业务成本（`counts_business_quota=true`）；
   - 被“每小时任意请求限频（hourly-any）”提前挡掉、未进入业务配额链路的请求：
     记为 `counts_business_quota=false`，以保持与配额计数语义一致。
3. **rollup 仅聚合 billable**
   - `rollup_token_usage_stats` 的 SQL 增加 `WHERE counts_business_quota = 1`，
     使 `token_summary_since` 以及所有 UI totals 自动变为业务成本口径。

### 历史数据处理

- 新增列默认值为 `1`，旧数据继续计入 UI 用量；
- 新增日志按真实口径写入，虚高会随时间逐步收敛。

### 结果语义

- `auth_tokens.total_requests` 仍保留“原始总请求次数（raw）”语义，用于管理视图与对账；
- UI 的 `today_total/month_total/all_total` 变为“业务成本用量（billable）”语义，
  与配额一致，可用于观察真实消耗。

## 对当前线上数据的结论

基于线上数据库快照：

- `auth_tokens.total_requests` 与 `auth_token_logs` 全量计数严格一致；
- 最近 24 小时内，所有活跃 token 的：
  - 聚合用量（`token_usage_buckets`，粒度 `hour`）与实际日志计数
    差值不超过若干次调用（`±4` 以内），符合“近似计数”预期；
  - 使用量远低于当前配置的小时 / 日限额（`100 / 500`），
    不存在因计数错误导致的误封或漏封；
- 月度聚合（`auth_token_quota.month_count`）对部分 token 明显偏小，
  反映的是“从启用配额逻辑起”的近似值，而非完整当月总数；在当前流量下，
  这不会影响限额判断，但后续若要将月度用量用于计费或严格审计，需要：
  - 要么回填历史数据；
  - 要么在文档与 UI 上明确标记为“近似值”，并只用于提示性展示。

结合你的要求——“限额允许适度误差，并优先降低资源占用”——当前实现的用量计算
与限额逻辑在设计上是合理的，且与线上数据吻合：小时 / 日限额可以安全依赖聚合
表，月度限额则需要视业务要求决定是否进一步精确化。

后续在引入 `auth_token_logs` 清理功能时，可以以本设计为基准：

- 保证配额逻辑继续只依赖 `token_usage_buckets` 与 `auth_token_quota`；
- 为 `auth_token_logs` 设置时间保留期与定期清理任务；
- 根据需要扩展新的聚合表，用于**图表与审计**，避免把配额判断绑死在大日志表上。

## 后续迁移计划（概要）

为实现“日志可清理、配额与用量展示依赖轻量聚合”的目标，迁移按以下阶段推进：

1. **引入新的聚合表 `token_usage_stats`**
   - 作用：按 `token_id + bucket_start + bucket_secs` 聚合 usage，用于 Token 详情页的 Usage Snapshot 与图表。
   - 建议字段：
     - `token_id`、`bucket_start`（对齐到整点）、`bucket_secs`（先固定为 3600）；
     - `success_count`、`system_failure_count`、`external_failure_count`、`quota_exhausted_count`。
   - 辅助索引：`(token_id, bucket_start DESC)`。

2. **一次性回填 `token_usage_stats`**
   - 从现有 `auth_token_logs` 中按小时聚合，填充最近 N 天（例如 30–90 天）的 usage。
   - 回填逻辑沿用当前 `fetch_token_usage_series` 中的分类规则，保证图表外观不变。

3. **增量维护策略**
   - 推荐：通过 `scheduled_jobs` 增加周期性 rollup 任务：
     - 周期性扫描 `auth_token_logs` 中 “自上次 rollup 以来的新行”，按小时聚合并 upsert 到 `token_usage_stats`；
     - 写入结束后更新 `last_rollup_ts`。

4. **切换读路径，脱离 `auth_token_logs`**
   - `fetch_token_summary_since`：从 `token_usage_stats`（或必要时 `request_logs`）聚合，替代按 token 直接扫 `auth_token_logs`。
   - `fetch_token_hourly_breakdown`：改为直接读取 `token_usage_stats` 中的小时桶。
   - `fetch_token_usage_series`：
     - 收紧 `bucket_secs` 取值（例如仅支持 3600 / 86400）；
     - 小时时间粒度直接读取 `token_usage_stats`；按日粒度在此基础上再聚合。
   - 保留基于 `auth_token_logs` 的接口仅限：
     - Token 详情页“最近请求记录”列表；
     - 公共 `/api/public/logs`。

5. **调整一致性迁移与统计的依赖关系**
   - `migrate_data_consistency()` 不再在每次启动时用 `auth_token_logs` 重算 `auth_tokens.total_requests`。
   - 后续如需矫正 `total_requests` / `last_used_at`，应通过一次性修复或显式的维护任务完成，而不是在有日志保留期的前提下自动依赖 `auth_token_logs`。

6. **启用 `auth_token_logs` 保留策略**
   - 在确认聚合表和新读路径稳定后，引入后台清理任务：
     - 仅依据时间保留策略清理日志，例如仅保留最近 N 天的 `auth_token_logs` 记录；
     - 不根据 access token 的启用 / 禁用 / 删除状态删除日志，以避免影响审计与追溯能力；
     - 按需触发 `VACUUM` 或使用 `auto_vacuum` 控制数据库文件大小。
   - 在此模式下：
     - 配额判断依旧依赖 `token_usage_buckets` 与 `auth_token_quota`；
     - 所有展示性用量数据来自聚合表或 `request_logs`；
     - `auth_token_logs` 仅作为“短期可观测性 + 最近记录列表”的数据源，在保留期内保持完整。
