# 多供应商搜索接入（SerpApi / Exa / Brave）

> 当前有效规范以本文为准；实现覆盖与当前状态见 `./IMPLEMENTATION.md`，关键演进与后续项见 `./HISTORY.md`。
> 设计正文见 `../../multi-provider-search-proxy.md`；上游合同与决策依据见 `.omo/drafts/serpapi-exa-brave-providers.md`
> 与 `docs/adr/0005-multi-provider-upstream-abstraction.md`。

## 背景 / 问题陈述

- Hikari 已具备 Tavily `/mcp` 代理与 `/api/tavily/*` HTTP façade：密钥池、共享 credit 钱包、
  request_logs 审计、高匿出站头策略与分段对账。
- SerpApi、Exa、Brave 的合同与 Tavily 存在真实接缝差异：密钥注入方式（query / header / Bearer）、
  耗尽信号（429 语义分流 / 402 tag / 402+422 码分）、用量同步来源（/account / 响应头被动采集 / 无 API）、
  以及 MCP 托管形态（托管直通 / 无托管）。
- 需要把这些供应商以纯增量方式接入同一套基础设施，同时保持 Tavily 请求路径字节级不变。

## 目标 / 非目标

### Goals

- 三条新增 REST façade：`/api/serpapi/*`、`/api/exa/*`、`/api/brave/*`，原生形状透传。
- 三个新增 MCP 入口：`/serpapi/mcp`（托管直通）、`/exa/mcp`、`/brave/mcp`（本地 facade）。
- provider-scoped 密钥池：获取 / 亲和 / transient backoff / quarantine / 恢复策略按商隔离。
- 共享 credit 钱包 + 按商按操作计价表（REST 1 / 1-1-2 / 1-1-2；SerpApi MCP 工具各 1）。
- 用量同步：SerpApi `/account` 主动车道；Brave `X-RateLimit-*` 月窗口被动采集；Exa typed unsupported。
- 管理/用户读面带 provider 维度（summary / logs facets+filter / token 日志 / key 详情）。
- 全链路脱敏与零计费校验探针；stub-only 测试策略。

### Non-goals

- 不改变 Tavily `/mcp`、`/api/tavily/*`、既有告警键、既有 header 白名单与 env 默认值。
- 不做跨供应商 fallback；不代理各商未列出的面（agent / websets / images / videos / Answers 等）。
- 不提供 Exa 托管 MCP 直通（Cloudflare 阻断观测 + agnost.ai 分析冲突，ADR 0005 记录延期）。
- 不做新供应商低余额降级（`api_key_low_quota_depletions` 保持 Tavily-only）。
- 不新增 Exa 团队级全局限流器（QPS 天花板只做文档与后续项）。
- 不新增 CLI 包装二进制；不新增 i18n 语言；不做流式透传。

## 范围（Scope）

### In scope

- 路由与认证：`src/server/provider_facade.rs`、`src/server/serve.rs` 的增量注册与 provider 解析。
- Provider 注册表：`src/providers/{id,spec}.rs`（ProviderId / ProviderSpec / 计价函数 / 恢复策略）。
- 分类与隐私：`src/analysis/provider_failure_classification.rs`、per-provider 头白名单与脱敏。
- REST/MCP 执行层：`src/tavily_proxy/proxy_provider_facade*.rs`、`src/server/{provider_mcp_serpapi, local_mcp_facade,provider_mcp_brave,provider_mcp_exa}.rs`。
- 数据层：主账本 migration v45（`api_keys` / `auth_token_logs` / `mcp_sessions` 的 provider 列）、
  sidecar `request_logs.provider` ALTER 路径、provider-scoped 池查询与恢复。
- 读面：summary / logs / token 日志的 provider 维度（additive DTO）。
- 文档与技能包：设计文档、本 spec 主题、ADR 0005、skills 包（T26）。

### Out of scope

- Exa 托管 MCP 直通、Exa agent/monitors/batches/websets、Brave images/videos/place/Answers、
  SerpApi async/archive/html 输出。
- 新供应商低余额降级与团队级全局限流（见 `HISTORY.md` 后续项）。
- 公共 docs-site 页面（T28 独立收敛；本主题只承载内部合同）。

## 需求（Requirements）

### MUST

- `GET`/`POST /api/serpapi/search`、`POST /api/exa/search`、`POST /api/exa/contents`、
  `POST /api/exa/answer`、`GET /api/brave/res/v1/web/search`、`GET /api/brave/res/v1/news/search`、
  `GET /api/brave/res/v1/llm/context` 必须注册为增量路由；`/serpapi/mcp`、`/exa/mcp`、`/brave/mcp`
  必须提供 MCP 入口，且 `/{provider}/mcp/*path` 子路径必须本地拒绝、不命中上游。
- 每个入口必须接受 `Authorization: Bearer th-...`，并接受各自的供应商原生槽
  （SerpApi query `api_key`、Exa 头 `x-api-key` 与 MCP query `exaApiKey`、Brave 头
  `X-Subscription-Token`）；客户端槽必须在转发与落盘前剥离。
- 上游密钥注入必须按商执行：SerpApi REST query `api_key`、SerpApi MCP `Authorization: Bearer`、
  Exa 头 `x-api-key`、Brave 头 `X-Subscription-Token`；Tavily-only 头（`Tavily-Api-Key`、
  `tavilyApiKey`、`X-Project-ID`）绝不能出现在任何非 Tavily 出站。
- 失败分类必须按商执行并接入既有健康机制：SerpApi 双 429（“run out of searches” → exhausted，
  其他 429 → 小时级 backoff）；Exa 402 tag 集合 → exhausted、429 遵守 `Retry-After`；
  Brave 422 码分（token invalid → auth-fail）、402 → exhausted、429 → 秒级 backoff。
- `Exa stream:true`（或任何真值 `stream`）必须在本地 typed 拒绝且零上游调用；
  Brave/Exa/SerpApi 的边界外操作同样本地拒绝。
- 计费必须复用共享 reservation/settlement：serpapi search=1；exa search=1/contents=1/answer=2；
  brave web=1/news=1/llm_context=2；SerpApi MCP 工具各 1；控制面方法（initialize/ping/tools/list/
  resources/notifications）非计费。
- 用量同步：SerpApi `/account` 车道（`GET {rest}/account?api_key=<secret>`，free）更新
  `quota_limit` / `quota_remaining` / `quota_renews_at` / `quota_synced_at`；
  Brave 月窗口被动采集（零额外请求、材料变化才写、`Limit=0` 绝不写 0）；Exa 返回 typed unsupported。
- Tavily 的 quota-sync 待同步选择、分代对账候选枚举与 research-drain 候选必须加 provider 守卫，
  非 Tavily 密钥不得进入 Tavily 出站。
- 低余额降级（`api_key_low_quota_depletions`）保持 Tavily-only；新供应商绝不写该表。
- 告警复用既有类型键并携带可选 `provider` 字段；Tavily/legacy 行不出现该键。
- `GET /api/logs` 必须支持 `?provider=` 过滤（NULL 行按 `tavily` 归并）与 provider facets；
  summary（+windows）与 token 日志必须提供 provider 维度（additive）。
- 测试只允许 stub/sandbox 上游；禁止命中生产端点。

### SHOULD

- request_logs 的 provider 行携带 provider kind（`api:serpapi/search`、`api:exa/*`、`api:brave/*`、
  `mcp:serpapi/*`、`mcp:brave/*`、`mcp:exa/*`）。
- 上游响应（含 Brave `X-RateLimit-*`）原样透传客户端，仅做脱敏。
- SerpApi `/account` 同步翻回 active 时写维护审计行；`account_status` 非 Active 只写备注且去重。
- admin validate 使用零计费探针并在对话框展示探针方法。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- REST 请求：解析 provider → 认证（Bearer/原生槽）→ 剥离客户端槽 → 闸门栈（HA fence、小时限频、
  reserved-credit、billing lock、`businessCalls1h`）→ 池内选 key → 按商注入密钥 → 上游调用 →
  分类 + `reconcile_key_health` → request_logs/token 日志（provider 维度）→ 业务成功 settle →
  原样返回上游响应。
- MCP 直通（SerpApi）：initialize 建立 `proxy_session_id` 并持久化 `mcp_sessions.provider='serpapi'`；
  follow-up 按 provider 作用域解析上游 session；`tools/call` 计费，控制面非计费；body 字节透传。
- 本地 facade（Exa/Brave）：`initialize`/`ping`/`tools/list`/`prompts/list`/`resources/*`/
  `notifications/*` 本地应答（SSE 包裹复用）；`tools/call` 翻译为上游 REST 调用（Exa POST、Brave GET），
  结果以 MCP `content`/`structuredContent` 返回，失败为 `isError=true` 并携带原生错误体。
- 用量同步：SerpApi 车道按 hot/cold 阈值在既有调度器内运行；Brave 在每次响应后 best-effort 采集；
  Exa 无主动轮询。

### Edge cases / errors

- 某商密钥全部不可用 → Hikari `502`（NoAvailableKeys 语义），不跨商 fallback。
- 上游 4xx/5xx 原样透传（除脱敏），同时按分类信号落地健康动作。
- `Limit=0` 月窗口：跳过写入（不写 0、不覆写 NULL）。
- Brave 错误响应通常无月窗口头 → 采集快速 no-op，不延长失败路径。
- SerpApi `/account` 解析失败（如 renewal date 不可解析）→ 保留既有 `quota_renews_at`（COALESCE），
  不误清空。
- 流式请求（Exa `stream:true`）→ 本地 typed `400`，mock 调用计数不变。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                        | 类型（Kind）  | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc）                                                 | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）                      |
| ----------------------------------- | ------------- | ------------- | -------------- | ------------------------------------------------------------------------ | --------------- | ------------------- | ---------------------------------- |
| `GET/POST /api/serpapi/search`      | HTTP route    | external      | New            | `../../multi-provider-search-proxy.md` §4.1                              | backend         | API clients         | Bearer 或 query `api_key`          |
| `POST /api/exa/search`              | HTTP route    | external      | New            | `../../multi-provider-search-proxy.md` §4.2                              | backend         | API clients         | Bearer 或头 `x-api-key`            |
| `POST /api/exa/contents`            | HTTP route    | external      | New            | `../../multi-provider-search-proxy.md` §4.2                              | backend         | API clients         | URL 提取；`stream` 拒绝            |
| `POST /api/exa/answer`              | HTTP route    | external      | New            | `../../multi-provider-search-proxy.md` §4.2                              | backend         | API clients         | 非流式回答                         |
| `GET /api/brave/res/v1/web/search`  | HTTP route    | external      | New            | `../../multi-provider-search-proxy.md` §4.3                              | backend         | API clients         | Bearer 或头 `X-Subscription-Token` |
| `GET /api/brave/res/v1/news/search` | HTTP route    | external      | New            | `../../multi-provider-search-proxy.md` §4.3                              | backend         | API clients         | 原生路径镜像                       |
| `GET /api/brave/res/v1/llm/context` | HTTP route    | external      | New            | `../../multi-provider-search-proxy.md` §4.3                              | backend         | API clients         | 查询驱动 grounding                 |
| `ANY /serpapi/mcp`                  | MCP route     | external      | New            | `../../multi-provider-search-proxy.md` §4.1                              | backend         | MCP clients         | 托管直通；Bearer 注入              |
| `ANY /exa/mcp`                      | MCP route     | external      | New            | `../../multi-provider-search-proxy.md` §4.2                              | backend         | MCP clients         | 本地 facade；3 工具                |
| `ANY /brave/mcp`                    | MCP route     | external      | New            | `../../multi-provider-search-proxy.md` §4.3                              | backend         | MCP clients         | 本地 facade；3 工具                |
| `quota_sync/serpapi`                | job type      | internal      | New            | `../../multi-provider-search-proxy.md` §4.1（`.omo/evidence/task-19-*`） | backend         | schedulers          | 代码完成未提交（验证通道故障）     |
| `brave_passive`                     | sample source | internal      | New            | `../../multi-provider-search-proxy.md` §4.3                              | backend         | quota samples       | 被动采集 source                    |
| `provider` 列 / 过滤 / facets       | DB + API      | internal      | Additive       | `../sqlite-write-lock-hardening/SPEC.md`（sidecar ALTER 路径）           | backend/web     | admin/user          | NULL 归并 `tavily`                 |

### 契约文档（按 Kind 拆分）

- HTTP 路由与 MCP 入口：`../../multi-provider-search-proxy.md` 的端点表与每商合同表为权威；
  逐操作请求/响应细节以 stub 合同测试为准（`tests/provider_facades_contract.rs`、`tests/provider_mcp_e2e.rs`）。
- 枚举契约：`../rebalance-mcp-gateway/SPEC.md` 的 Amendment 2026-10-09（additive 记录 provider-scoped
  `upstream_operation` 词表）。
- 技能包：`skills/{serpapi,exa,brave}-hikari-*/SKILL.md` 与 `skills/README.md`。

## 验收标准（Acceptance Criteria）

- Given 任一新增 REST 入口与有效 `th-` 令牌
  When 客户端以 Bearer 或原生槽发起请求
  Then 返回原生形状上游响应（成功），且 mock 断言客户端槽未出现在上游。

- Given 非 Tavily 出站请求
  When 观察 mock 上游
  Then 不存在 `x-project-id`、`Tavily-Api-Key`、`tavilyApiKey`；池内密钥仅出现在该商原生槽。

- Given Exa `/api/exa/answer` 请求体 `stream:true`
  When 请求进入 façade
  Then 本地返回 typed `400`，mock 调用计数为 0。

- Given SerpApi 429 body 含 “run out of searches”（原始 JSON 或 SSE 包裹）
  When 请求经 façade
  Then 密钥标记 exhausted 且上游 429 原样透传；其他 429 只触发小时级 transient backoff。

- Given Exa 402 tag `NO_MORE_CREDITS`
  When 请求经 façade
  Then 密钥标记 exhausted，响应原样透传；下一次成功调用可恢复。

- Given Brave 422 `SUBSCRIPTION_TOKEN_INVALID`
  When 请求经 façade/MCP
  Then 密钥进入 quarantine（invalid_api_key），无上游重试。

- Given Brave 成功响应携带 `X-RateLimit-*` 月窗口
  When 响应返回
  Then 客户端可见同样头，且 key 的 `quota_remaining` 按材料变化更新（零额外请求）。

- Given 混合供应商密钥池
  When 请求某商 façade
  Then 仅租用该商 key；目标商无可用 key 时返回 502 且其他商不受影响。

- Given 令牌余额不足以覆盖下一次业务调用
  When 请求任意供应商
  Then 本地 `429 quota_exhausted`，零上游调用。

- Given 新供应商 exhausted、harvest、restore 流程
  When 检查 `api_key_low_quota_depletions`
  Then 无任何新供应商行（Tavily-432 正向对照可写）。

## 验收清单（Acceptance checklist）

- [x] 核心路径的长期行为已被明确描述。
- [x] 关键边界/错误场景已被覆盖。
- [x] 涉及的接口/契约已写清楚或明确为 `None`。
- [x] 相关验收条件已经可以用于实现与 review 对齐。
- [x] 已记录未提交车道（`quota_sync/serpapi`）的诚实状态与后续项。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- 后端：`tests/provider_facades_contract.rs`（REST 合同）、`tests/provider_mcp_e2e.rs`（MCP 生命周期）、
  `src/tests/provider_*`（分类、池作用域、隐私、读面）、`src/server/tests/provider_*`（路由、admin、读面）。
- 全部新测试注册在 `scripts/ci_backend_test_manifest.json`；golden Tavily 套件保持不变。
- stub-only：`mock_serpapi` / `mock_exa` / `mock_brave`；禁止生产端点。

### Quality checks

- `cargo fmt --check`
- `cargo clippy --locked -j 2 -- -D warnings`
- 受影响 shard 的 `python scripts/ci_backend_tests.py run-shard --id <shard>`
- `bunx --bun dprint check 'docs/**'`

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：供应商条款限制转售/池化/多账号，运营者需按 §8 注记自行确认（文档已给出边界）。
- 风险：Exa QPS 为团队级，池化不放大上限；v1 无全局限流器（后续项）。
- 假设：SerpApi `/account` 保持免费且字段稳定；Brave 月窗口头语法保持位置配对。
- 开放问题：本地 facade 的独立 `gateway_mode` 词表待 ratified（见 `HISTORY.md` FU-8）。

## 参考（References）

- `../../multi-provider-search-proxy.md`
- `../../../docs/adr/0005-multi-provider-upstream-abstraction.md`
- `../rebalance-mcp-gateway/SPEC.md`（Amendment 2026-10-09）
- `../upstream-credits-billing/SPEC.md`、`../low-quota-432-depletion/SPEC.md`、`../request-kind-canonicalization-lossless-history/SPEC.md`
- `.omo/evidence/task-{2..26}-serpapi-exa-brave-providers.*`、`.omo/evidence/task-19-serpapi-exa-brave-providers.txt`
