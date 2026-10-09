# 多供应商搜索中转设计（`/api/serpapi/*`、`/api/exa/*`、`/api/brave/*` 与 `/{provider}/mcp`）

本文档描述 Tavily Hikari 在既有 Tavily 表面之外新增的 **SerpApi、Exa、Brave Search** 三条搜索供应商链路，
覆盖各自的 REST façade、MCP 入口、密钥池接入、计费、健康语义、用量同步、脱敏与合规边界。

本文档是当前实现的接入与维护基线，结构与 `docs/tavily-http-api-proxy.md` 保持一致。

上游合同与决策依据见 `.omo/drafts/serpapi-exa-brave-providers.md`（含 D1-D10 决策与逐商合同摘录）
与 `docs/adr/0005-multi-provider-upstream-abstraction.md`；实现覆盖与文件锚点见
`docs/specs/multi-provider-search-integration/IMPLEMENTATION.md`，演进与后续项见同目录 `HISTORY.md`。

---

## 1. 背景与目标

现状：

- Hikari 已通过 `TavilyProxy` 提供 Tavily `/mcp` 代理与 `/api/tavily/*` HTTP façade，
  具备密钥池亲和 / LRU 调度、健康状态（432）、共享 credit 钱包、request_logs 审计与高匿出站头策略。
- 供应商差异集中在少数真实接缝上：上游 URL 构建、密钥注入方式、耗尽信号、工具命名、用量同步来源。
  这些差异此前全部硬编码为 Tavily 语义。

需求：

- 在不改变任何 Tavily 行为前提下，把 SerpApi、Exa、Brave 作为同级供应商接入同一套基础设施：
  - 每商独立密钥池（provider-scoped 获取、亲和、退避、隔离）；
  - 共享同一 credit 钱包与按操作计价表；
  - 原生形状 REST façade，`th-` 访问令牌可放入各供应商原生鉴权槽；
  - MCP 入口（SerpApi 直通官方托管 MCP；Exa、Brave 由 Hikari 本地 facade 提供）；
  - 管理/用户 API 与日志携带 provider 维度。

目标：

1. 在 `/api/serpapi/*`、`/api/exa/*`、`/api/brave/*` 下提供原生 HTTP façade。
2. 在 `/serpapi/mcp`、`/exa/mcp`、`/brave/mcp` 下提供 MCP 入口。
3. 对内复用现有 KeyStore、credit 钱包、reservation/settlement、request_logs 与告警体系。
4. 所有出站请求遵守与 Tavily 同级的高匿白名单；客户端令牌与池内密钥绝不泄露到日志或上游。
5. Tavily `/mcp` 与 `/api/tavily/*` 字节级不变（golden 套件回归约束）。
6. 任何请求只命中被寻址的供应商，不做跨供应商 fallback/仲裁。

---

## 2. 端点总览

新增入口全部独立注册，Tavily 的 `/mcp` 与 `/api/tavily/*` 保持原注册不变：

- 注册表：`src/server/provider_facade.rs:88-159`（REST 路由表）、`:163-167`（MCP 入口表）。
- 路由挂载：`src/server/provider_facade.rs:218-245`，由 `src/server/serve.rs:566` merge 进主 router。

| Method   | Path                            | 上游端点                                              | 客户端鉴权槽（`th-` 令牌）                                   | 说明                                   |
| -------- | ------------------------------- | ----------------------------------------------------- | ------------------------------------------------------------ | -------------------------------------- |
| GET/POST | `/api/serpapi/search`           | `GET {SERPAPI_UPSTREAM}/search`                       | `Authorization: Bearer` 或 query `api_key`                   | 搜索；POST 为 JSON 体到原生 query 映射 |
| POST     | `/api/exa/search`               | `POST {EXA_UPSTREAM}/search`                          | `Authorization: Bearer` 或头 `x-api-key`                     | 搜索                                   |
| POST     | `/api/exa/contents`             | `POST {EXA_UPSTREAM}/contents`                        | `Authorization: Bearer` 或头 `x-api-key`                     | URL 内容提取（extract 等价）           |
| POST     | `/api/exa/answer`               | `POST {EXA_UPSTREAM}/answer`                          | `Authorization: Bearer` 或头 `x-api-key`                     | 带引用回答（非流式）                   |
| GET      | `/api/brave/res/v1/web/search`  | `GET {BRAVE_UPSTREAM}/res/v1/web/search`              | `Authorization: Bearer` 或头 `X-Subscription-Token`          | Web 搜索（原生路径镜像）               |
| GET      | `/api/brave/res/v1/news/search` | `GET {BRAVE_UPSTREAM}/res/v1/news/search`             | `Authorization: Bearer` 或头 `X-Subscription-Token`          | 新闻搜索                               |
| GET      | `/api/brave/res/v1/llm/context` | `GET {BRAVE_UPSTREAM}/res/v1/llm/context`             | `Authorization: Bearer` 或头 `X-Subscription-Token`          | LLM Context（查询驱动预抽取片段）      |
| ANY      | `/serpapi/mcp`                  | `{SERPAPI_MCP_UPSTREAM}`（托管 MCP）                  | `Authorization: Bearer` 或 query `api_key`                   | 字节直通托管 MCP                       |
| ANY      | `/exa/mcp`                      | 本地 facade（`POST /search`、`/contents`、`/answer`） | `Authorization: Bearer`、头 `x-api-key` 或 query `exaApiKey` | 本地 MCP facade                        |
| ANY      | `/brave/mcp`                    | 本地 facade（`GET /res/v1/...`）                      | `Authorization: Bearer` 或头 `X-Subscription-Token`          | 本地 MCP facade                        |

补充约定：

- `/{provider}/mcp/*path` 子路径由各 provider 的拒绝处理器本地处理（`src/server/provider_facade.rs:240-244`），
  不会落入 SPA HTML fallback，也不会命中上游。
- Brave 的注册路径是完整原生镜像 `/api/brave/res/v1/...`，便于 base-URL-override 客户端直接改 base；
  request_logs 中的 provider kind 使用既有 canonical 路径键（`api:brave/{web,news,llm-context}`，
  见 `src/analysis.rs:915-930`），`auth_token_logs` 保留完整注册路径。
- SerpApi 的 POST 变体是便捷映射：JSON 体字段（`q`、`engine`、`num`、`start`、`output`、`no_cache`、
  `location`、`gl`、`hl`、`json_restrictor` 等）在本地合并为原生 query 后再出站；GET 变体直接透传 query。

### 2.1 原生兼容与边界约定

各供应商的原生面并不等价（SerpApi 只有搜索族；Exa 有 search/contents/answer；Brave 有搜索族 +
LLM Context，没有 URL 提取/爬取）。Hikari 的下游合同只覆盖各商原生可用、且当前代理可正确计费和审计的能力：

- 支持：上表 10 个入口所列操作，以及 SerpApi 托管 MCP 的 `search`、`search_table`、`search_dashboard`
  工具与 `resources` 读取（`resources/*`、`tools/list` 等控制面非计费）。
- 不支持（本地 typed 拒绝，不会打到上游）：
  - Exa `stream:true`（或任何真值 `stream`）；请求在本地返回 typed `400`
    （错误信息注明 “streaming not supported; omit stream or set false”）。这是既有 free-account
    boundary 规则的镜像，`stream` 字段不会发生 SSE 透传。
  - Exa `/findSimilar`、`/agent/*`、`/monitors`、`/batches`、`/v0/websets/*`、`/chat/completions` 兼容面。
  - Brave `images`、`videos`、`local/place`、`suggest`、`spellcheck`、`summarizer`、`chat/completions`（Answers）。
  - SerpApi async 模式、search-archive、`locations` 端点、`output=html` 透传。
  - 跨供应商自动 fallback；某商密钥全部不可用时返回 Hikari `502`（NoAvailableKeys 语义），
    不会改投其他供应商。
  - 除脱敏外的任何响应体改造；上游 4xx/5xx 一律原样透传。

---

## 3. 认证与鉴权设计

### 3.1 客户端令牌与原生槽矩阵

所有新入口统一要求 Hikari 访问令牌（`th-<id>-<secret>`），并额外接受“客户端本就可能携带”的原生槽。
解析实现：`src/server/provider_facade.rs:278-376`（`authenticate_provider_facade_request`）。

| 入口                                | 通用槽                  | 原生头槽               | 原生 query 槽           |
| ----------------------------------- | ----------------------- | ---------------------- | ----------------------- |
| `/api/serpapi/search`               | `Authorization: Bearer` | -                      | `api_key`               |
| `/serpapi/mcp`                      | `Authorization: Bearer` | -                      | `api_key`               |
| `/api/exa/{search,contents,answer}` | `Authorization: Bearer` | `x-api-key`            | -                       |
| `/exa/mcp`                          | `Authorization: Bearer` | `x-api-key`            | `exaApiKey`（MCP 专用） |
| `/api/brave/res/v1/*`               | `Authorization: Bearer` | `X-Subscription-Token` | -                       |
| `/brave/mcp`                        | `Authorization: Bearer` | `X-Subscription-Token` | -                       |

解析顺序（首个非空命中）：`Authorization: Bearer` > 原生头槽 > 原生 query 槽 > Exa MCP query 槽。
命中后统一走既有 `validate_access_token`，失败一律返回 Hikari 形状的 `401`（绝不返回供应商形状的错误）。
`DEV_OPEN_ADMIN=true` 时保留本地 dev fallback（仅本地调试，生产严禁依赖）。

### 3.2 剥离与记录语义（strip-before-forward / strip-before-log）

- 转发前：`Authorization`、供应商原生头槽、`X-Hikari-Routing-Key`、原生 query 槽全部从客户端请求中剥离；
  上游只收到池内选中的真实密钥（按各商注入方式，见 §4）。
- 日志前：持久化的 query/headers 只包含剥离后的文本；客户端 `th-` 令牌与池内密钥都不落盘。
  展示层另有短语级脱敏（见 §5.2）。
- `X-Hikari-Routing-Key` 与 `/api/tavily` 保持同等接受度（M10），但新 façade 使用既有 token/user
  亲和策略，该头在新面内惰性（不存储、不转发）。
- 相关规范：[`upstream-agnostic-api-rebalance/SPEC.md`](specs/upstream-agnostic-api-rebalance/SPEC.md)。

### 3.3 配额与计费闸门

REST façade 在出站前复用与 `/api/tavily` 相同的闸门栈（`src/server/provider_facade.rs:378-427` 及后续）：
HA business fence、每小时任意请求限频、reserved-credit 预检查、billing lock、account 级
`businessCalls1h` reservation。被拒绝时返回既有 `429 quota_exhausted` 形状并写入 token 日志，
不会命中上游。业务成功后才按 §4 的 credit 表 settle（共享同一钱包，无按商分账）。

---

## 4. 每商详细设计

所有新供应商共享同一套执行骨架：

- 失败分类器：`src/analysis/provider_failure_classification.rs`（T3）输出统一信号
  `Ok / Exhausted / AuthFailed / RateLimited{retry_after_secs} / ClientError / ServerTransient`，
  再由既有 `reconcile_key_health` 落地状态（隔离、exhausted、transient backoff、恢复）。
- request_logs：每条 provider 行携带 `provider`，upstream 状态写入既有 `tavily_status_code` 列
  （列名保留，语义为 “上游状态码”）。
- 健康动作与告警：复用既有告警类型键，payload 增加可选 `provider` 字段（Tavily/legacy 行省略该键，wire 形状不变）。

### 4.1 SerpApi

**端点与 MCP**

| 入口                  | 上游                                 | 密钥注入                                | 备注                                     |
| --------------------- | ------------------------------------ | --------------------------------------- | ---------------------------------------- |
| `/api/serpapi/search` | `GET {SERPAPI_UPSTREAM}/search`      | query `api_key=<pooled>`（REST 原生槽） | GET 透传；POST 便捷映射                  |
| `/serpapi/mcp`        | `{SERPAPI_MCP_UPSTREAM}`（托管 MCP） | `Authorization: Bearer <pooled>`        | 直通；会话 ID 翻译；`provider='serpapi'` |

MCP 工具：`search`、`search_table`、`search_dashboard`（各 1 credit，仅 `tools/call` 计费）；
`initialize`、`ping`、`tools/list`、`resources/*`、`notifications/*` 非计费。
分类器对工具名做 provider 前缀化（`mcp:serpapi/search` 等），避免与 Tavily canonical 名冲突。

**错误语义（双 429 分流）**

| 上游状态                                                                 | 分类信号                | 密钥健康动作                                           |
| ------------------------------------------------------------------------ | ----------------------- | ------------------------------------------------------ |
| 400 / 410                                                                | ClientError             | 无惩罚，原样透传                                       |
| 401 / 403                                                                | AuthFailed              | 进入既有 quarantine 机制（invalid_api_key）            |
| 429 + `"run out of searches"`（大小写不敏感，兼容原始 JSON 与 SSE 包裹） | Exhausted               | 标记 exhausted；恢复策略 RenewalDate                   |
| 其他 429                                                                 | RateLimited（小时尺度） | transient backoff（默认 3600s，Retry-After 30..=3600） |
| ≥500                                                                     | ServerTransient         | 无惩罚                                                 |

**用量同步（`/account` 车道）**：

- 设计锁定（T19）：新 job 类型 `quota_sync/serpapi`，复用既有 hot/cold 待同步阈值与 remote-attempt
  lease/gate；`GET {rest}/account?api_key=<secret>`（官方文档标注免费、不计次）映射：
  `quota_limit=searches_per_month`、`quota_remaining=total_searches_left`、
  `quota_renews_at=plan_renewal_date`（UTC 零点）、`quota_synced_at=now`。
- 恢复：`reset_monthly(ProviderId::Serpapi)` 复用既有 store 策略，仅在 `quota_renews_at <= now`
  时把 exhausted 翻回 active，并写维护审计行。
- `account_status != "Active"` 仅写维护记录备注，不自动禁用、不 quarantine。
- 状态说明：该车道代码在 T19 分支完成（未提交），因验证通道（WSL 主机）故障暂停在 G5/G6 门禁；
  设计已冻结，详见 `.omo/evidence/task-19-serpapi-exa-brave-providers.txt` 与
  `docs/specs/multi-provider-search-integration/HISTORY.md`。

**MCP 模式理由**：SerpApi 官方托管 MCP 可用且隐私面无阻断（不同于 Exa），直通可保留原生
`search/search_table/search_dashboard` 工具与 `serpapi://engines` 资源语义；Hikari 负责会话翻译与计费。

### 4.2 Exa

**端点与 MCP**

| 入口                | 上游                           | 密钥注入                 | 备注                          |
| ------------------- | ------------------------------ | ------------------------ | ----------------------------- |
| `/api/exa/search`   | `POST {EXA_UPSTREAM}/search`   | 头 `x-api-key: <pooled>` | 搜索                          |
| `/api/exa/contents` | `POST {EXA_UPSTREAM}/contents` | 头 `x-api-key: <pooled>` | URL 提取；`ids`/`urls` 二选一 |
| `/api/exa/answer`   | `POST {EXA_UPSTREAM}/answer`   | 头 `x-api-key: <pooled>` | 非流式回答                    |
| `/exa/mcp`          | 本地 facade                    | 头 `x-api-key: <pooled>` | 3 个工具，SSE 包裹复用        |

本地 MCP 工具：`web_search_exa`（1 credit）、`web_fetch_exa`（1 credit，`urls[]` XOR `ids[]`）、
`exa_answer`（2 credit）；工具名与 Exa 托管注册表默认值对齐，便于客户端提示词迁移。
`stream` 永远不向上游发送（省略或 `false`）。

**错误语义**

| 上游状态                                                                   | 分类信号        | 密钥健康动作                                    |
| -------------------------------------------------------------------------- | --------------- | ----------------------------------------------- |
| 401 `INVALID_API_KEY`                                                      | AuthFailed      | quarantine（invalid_api_key）                   |
| 402 tag ∈ {NO_MORE_CREDITS, API_KEY_BUDGET_EXCEEDED, TEAM_BUDGET_EXCEEDED} | Exhausted       | 标记 exhausted；SuccessOnly（成功即恢复或手动） |
| 402 其他 tag / 403                                                         | ClientError     | 无惩罚                                          |
| 429 `RATE_LIMIT_EXCEEDED`                                                  | RateLimited     | 遵守 `Retry-After`（30..=300 分钟级阶梯）       |
| 503 `SERVICE_OVERLOADED`                                                   | ServerTransient | 无惩罚（不计费）                                |
| `stream:true`（本地拦截）                                                  | ClientError     | 无惩罚；零上游调用                              |

**用量同步**：无实时余额 API（官方 team-management usage 只提供历史成本，职责不同），
admin `sync-usage` 返回 typed `409 unsupported`；额度按本地计数管理。

**MCP 模式理由（ADR 0005）**：Exa 托管 MCP `mcp.exa.ai` 被 Cloudflare 前置（直连曾观测到
“Sorry, you have been blocked”），且其服务端向第三方 `api.agnost.ai` 上报分析，与本站高匿姿态冲突；
因此 v1 仅提供本地 facade，托管直通延期（记录于 ADR 0005，后续决策需显式重开）。

### 4.3 Brave

**端点与 MCP**

| 入口                            | 上游                                      | 密钥注入                            | 备注               |
| ------------------------------- | ----------------------------------------- | ----------------------------------- | ------------------ |
| `/api/brave/res/v1/web/search`  | `GET {BRAVE_UPSTREAM}/res/v1/web/search`  | 头 `X-Subscription-Token: <pooled>` | 原生路径镜像       |
| `/api/brave/res/v1/news/search` | `GET {BRAVE_UPSTREAM}/res/v1/news/search` | 头 `X-Subscription-Token: <pooled>` | 新闻               |
| `/api/brave/res/v1/llm/context` | `GET {BRAVE_UPSTREAM}/res/v1/llm/context` | 头 `X-Subscription-Token: <pooled>` | 查询驱动 grounding |
| `/brave/mcp`                    | 本地 facade                               | 头 `X-Subscription-Token: <pooled>` | 3 个工具           |

本地 MCP 工具：`brave_web_search`（1 credit）、`brave_news_search`（1 credit）、
`brave_llm_context`（2 credit）。上游响应（含 `X-RateLimit-*` 头）原样透传客户端。

**错误语义（422 分流）**

| 上游状态                                                          | 分类信号        | 密钥健康动作                                   |
| ----------------------------------------------------------------- | --------------- | ---------------------------------------------- |
| 422 `SUBSCRIPTION_TOKEN_INVALID`                                  | AuthFailed      | quarantine（invalid_api_key）                  |
| 422 `VALIDATION` 且 `meta.errors[].loc` 含 `x-subscription-token` | AuthFailed      | 同上                                           |
| 422 `VALIDATION`（其他 loc）                                      | ClientError     | 无惩罚（客户端参数错误）                       |
| 402 `QUOTA_LIMITED`（或裸 402）                                   | Exhausted       | 标记 exhausted；月窗口（RateLimitResetWindow） |
| 429 `RATE_LIMITED`（或裸 429）                                    | RateLimited     | 短退避（默认 2s，钳制 1..=30）                 |
| ≥500                                                              | ServerTransient | 无惩罚                                         |

**用量同步（被动采集）**：

- 每个 façade/MCP 响应解析 `X-RateLimit-Policy` 中的月窗口（`w=2592000`）并与
  `X-RateLimit-Limit/Remaining/Reset` 按位置配对（`src/tavily_proxy/proxy_provider_facade_brave.rs:198-258`）。
- 仅在“材料变化”（首次、remaining 变化、limit/reset 窗口变化）时经 `record_quota_sync_sample`
  写入，source 固定 `brave_passive`；重复值不写，失败路径不额外延迟（best-effort、可跳过）。
- 零额外请求：采集只读取响应已缓冲的头。
- 已记录决策（T14/T19）：月窗口 `Limit=0`（无限额）在 `record_quota_sync_sample` 的 `i64` 非空接缝上
  无法表达为 NULL，因此映射为“跳过写入”，绝不持久化 `0`，新建 key 的 NULL `quota_limit` 保持不变；
  该限制与决策详见 `HISTORY.md` 后续项 FU-7。

**MCP 模式理由**：Brave 无官方托管 MCP（官方 MCP 为自托管形态），因此仅本地 facade。

### 4.4 Tavily 不变性

Tavily 的 `/mcp`、`/api/tavily/*`、用量同步（`TAVILY_USAGE_BASE/usage`、`quota_sync/hot` 与
`quota_sync/manual` 车道）与既有三段 header 白名单保持字节级不变（golden 套件回归约束）。
provider 化改造只新增旁路，不回写 Tavily 请求路径。

---

## 5. 安全与脱敏

### 5.1 出站头白名单

各 façade 使用独立白名单（`src/analysis.rs` 内 `sanitize_serpapi_http_headers_inner` /
`sanitize_exa_http_headers_inner` / `sanitize_brave_http_headers_inner`）：

| 供应商  | 允许转发的头                                                                |
| ------- | --------------------------------------------------------------------------- |
| SerpApi | `accept`、`accept-encoding`、`content-type`                                 |
| Exa     | `accept`、`accept-encoding`、`content-type`                                 |
| Brave   | `accept`、`accept-encoding`、`content-type`、`cache-control`、`api-version` |

- `User-Agent` 一律不转发（与 Tavily 高匿规则一致）。
- 客户端密钥槽（`api_key` query、`x-api-key`、`X-Subscription-Token`）在白名单层即被丢弃；
  池内密钥在 façade 层注入。
- SerpApi MCP 直通复用既有 Control MCP 白名单
  （`accept`、`accept-encoding`、`cache-control`、`content-type`、`last-event-id`、
  `mcp-protocol-version`、`mcp-session-id`、`pragma`）。
- 既有三段 Tavily 白名单不改动（golden 断言精确头集合）。
- 参考：[`high-anonymity-proxy.md`](high-anonymity-proxy.md)。

### 5.2 密钥剥离与日志脱敏

- strip-before-forward/strip-before-log（M17）：客户端 `th-` 令牌与原生槽值在两处同时剥离，
  持久化 query/headers 永不含其原文。
- 持久化兜底：`redact_api_key_bytes`（`src/analysis.rs:2668`）在 request_logs 写入前对
  各商密钥形状做通用脱敏。
- 展示层短语脱敏（`src/server/handlers/public.rs:794-835`）覆盖：
  - query 解析名：`tavilyapikey=`、`api_key=`（大小写不敏感）；
  - 头短语：`tavily-api-key`、`x-api-key`、`x-subscription-token`。
- 传输错误信息不得回显带密钥的 URL（SerpApi `/account` 同步路径专门映射为字面错误）。

### 5.3 X-Project-ID 仅 Tavily

`X-Project-ID` 是 Tavily 专属身份策略（透传 / 固定 / accessToken HMAC 伪名）。
`apply_upstream_project_id_header_for_provider`（`src/tavily_proxy/proxy_request_limits.rs:1737`）
在 5 个共享 executor 出口（`proxy_http_and_logs.rs:333/681/930/1204/1488`）以 provider 参数门控：
非 Tavily 出口直接剥离伪造值、绝不注入。新供应商的 outbound 不存在 `x-project-id`。

### 5.4 零计费校验探针（M13）

`POST /api/keys/validate` 对各商使用零/低成本探针，隐藏计费探针被禁止：

| 供应商  | 探针                                    | 判据                                                             |
| ------- | --------------------------------------- | ---------------------------------------------------------------- |
| Tavily  | 既有行为不变                            | 不变                                                             |
| SerpApi | `GET /account?api_key=`（免费、不计次） | 返回套餐字段；无效密钥 401                                       |
| Exa     | `POST /contents` 空体 `{}`              | 401 = 无效；400/422 = 有效（校验类错误不计费）                   |
| Brave   | `GET /res/v1/web/search` 不带 `q`       | 422 `SUBSCRIPTION_TOKEN_INVALID` = 无效；422 `VALIDATION` = 有效 |

---

## 6. 与现有表面的关系

- **Rebalance / Reconciliation / Research drain**：分段对账（accessToken HMAC、结算、Research 轮询）
  与 research-drain 保持 Tavily-only。T19 为其待处理 key 选择与候选枚举增加
  `provider='tavily'` / NOT EXISTS 守卫，非 Tavily 密钥不可能进入 Tavily 出站链路
  （设计已锁定；该车道随 T19 未提交，状态见 §4.1 与 `HISTORY.md`）。
- **KeyPool**：获取、亲和、transient backoff、quarantine、低余额降级全部 provider 化；
  `reset_monthly` 按 RecoveryPolicy 分流（Tavily UTC 月；SerpApi 续费日；Exa 成功恢复；Brave 月窗口）。
- **低余额降级（M8）**：`api_key_low_quota_depletions` 仍严格 Tavily-only（432 门不动）；
  新供应商绝不写该表（负向断言在测试矩阵中固定）。
- **告警**：沿用既有类型键（`api_key_exhausted`、`upstream_key_blocked`、
  `upstream_rate_limited_429` 等），payload 增加可选 `provider` 字段；Tavily/legacy 行不出现该键，
  wire JSON 保持字节形状。
- **读面（T20）**：`GET /api/summary`（+`/windows`）新增按 provider 分解；`GET /api/logs` 支持
  `?provider=` 过滤与 facets；用户 token 日志携带 provider；NULL/legacy 行按 `tavily` 归并。
  DashboardReadModel / AlertProjection 保持 provider-agnostic（M18：不允许把不同单位的用量混入同一序列）。
- **数据库**：`api_keys`、`auth_token_logs`、`mcp_sessions` 的 provider 列由主账本 migration v45 拥有；
  `request_logs.provider` 走 observability sidecar 的 ALTER 路径（两者机制不混用）。

---

## 7. 集成指南约定

### 7.1 Base URL 与鉴权槽

| 供应商  | HTTP Base（Hikari 侧）       | 原生鉴权槽                         | MCP 入口       |
| ------- | ---------------------------- | ---------------------------------- | -------------- |
| Tavily  | `https://<host>/api/tavily`  | body `api_key` / Bearer            | `/mcp`         |
| SerpApi | `https://<host>/api/serpapi` | query `api_key` / Bearer           | `/serpapi/mcp` |
| Exa     | `https://<host>/api/exa`     | 头 `x-api-key` / Bearer            | `/exa/mcp`     |
| Brave   | `https://<host>/api/brave`   | 头 `X-Subscription-Token` / Bearer | `/brave/mcp`   |

所有槽位填写 Hikari 访问令牌（`th-<id>-<secret>`），而不是各商官方 key。

### 7.2 Cherry Studio 与客户端映射

- Cherry Studio 的 Tavily 指南保持既有 `docs/tavily-http-api-proxy.md` §8 约定不变。
- SerpApi：Provider 选 SerpApi，API 地址填 `https://<host>/api/serpapi`，API 密钥填 `th-...`
  （原生 `api_key` query 槽兼容）。
- Exa：API 地址填 `https://<host>/api/exa`，API 密钥填 `th-...`（客户端以 `x-api-key` 头携带时同样可识别）。
- Brave：Provider 选 Brave 时 Base 映射为 `https://<host>/api/brave`（完整 `/res/v1/...` 路径在下游拼接后
  与服务端注册路径精确匹配）。
- 面向终端用户的设置示意界面（CherryStudioMock）沿用 `docs/tavily-http-api-proxy.md` §8.2 的静态/禁用组件约定，
  新增文案只做复制级补充，不重做视觉结构。

### 7.3 Agent Skills 与公共文档

- 每操作技能包（T26 已落地）：`skills/serpapi-hikari-search`、`skills/exa-hikari-search`、
  `skills/exa-hikari-contents`、`skills/exa-hikari-answer`、`skills/brave-hikari-search`、
  `skills/brave-hikari-news`、`skills/brave-hikari-llm-context`；
  `skills/tavily-hikari-best-practices` 增加跨供应商对比；`skills/README.md` 维护 Base/鉴权槽矩阵。
- 本文档不重复完整 API 参考：逐操作请求/响应细节以技能包与 stub 合同测试为准
  （`tests/provider_facades_contract.rs`、`tests/provider_mcp_e2e.rs`）。
- 公共 docs-site 页面（configuration-access / http-api-guide / faq）由 T28 收敛本文档面向上游用户的章节。

---

## 8. 边界与 ToS 注记

### 8.1 边界汇总

| 供应商  | 不做                                                          | 说明                                      |
| ------- | ------------------------------------------------------------- | ----------------------------------------- |
| SerpApi | async、search-archive、locations、`output=html`               | 仅同步搜索族 + 托管 MCP 3 工具            |
| Exa     | 流式、agent/monitors/batches/websets、托管 MCP 直通           | `stream:true` 本地 typed 400；本地 facade |
| Brave   | URL 提取/爬取、images/videos/place/suggest/summarizer/Answers | 仅 web/news/llm_context                   |
| 全部    | 跨供应商 fallback；请求体内响应改造；SSE 透传                 | 一个请求只命中被寻址供应商                |

### 8.2 速率与容量

- **Exa QPS 是团队级**（search/answer 约 10 QPS，覆盖该团队全部 key）：多 key 池化不会放大 QPS。
  v1 保留按 key 退避（遵守 `Retry-After`），不新增全局限流器；provider 级冷却章（团队级天花板）
  列为后续项（M12，见 `HISTORY.md`）。
- Brave 每 plan 最多 10 key；429 为秒级限速，退避后自然恢复。

### 8.3 数据保留（Brave 操作者注记）

Brave ToS §3(b) 禁止长期保留搜索结果数据。Hikari 的现有 body-retention profiles 是操作者杠杆：
如需满足合规，应在管理端收紧 `request_logs` 体保留；本项目不新增按供应商分叉的保留代码，
仅保留为操作者策略注记。

### 8.4 法律与运营注记（全部供应商）

- 三家条款都限制转售、密钥共享与多账号轮换：SerpApi §2 / §6、Exa 不可转让
  非再许可许可、Brave §3(b)(v)/(xii) 与 §4(a)。
- 本产品以 token 计量转售访问能力，运营者需自行确认与各条款的适配；
  “同一组织自有密钥、在已购配额内池化”是文档化边界；多账号刷量、转售裸 key 不在支持范围。
- 测试与 CI 只允许 stub/sandbox 上游；既有 “never hit production endpoints” 规则扩展至
  SerpApi/Exa/Brave（AGENTS.md incident rule）。
