# 上游不可知 API 负载均衡（#cp8s9）

> 当前有效规范以本文为准；实现覆盖与当前状态见 `./IMPLEMENTATION.md`，关键演进原因见 `./HISTORY.md`。

## 背景 / 问题陈述

- MCP Rebalance 已经具备全池候选排序、短期 429/cooldown 避让与热度削峰。
- 普通 `/api/tavily/*` 仍以 Tavily `X-Project-ID` 项目亲和和 user/token primary affinity 为主，缺省 API 流量容易长期压在同一上游 key。
- 需要把 API 负载均衡抽成 Hikari 自己的通用能力，避免核心选路依赖某个上游供应商的 header 或 endpoint 语义。

## 目标 / 非目标

### Goals

- 为普通 `/api/tavily/*` 请求引入上游不可知的通用 API selector。
- 默认无 routing key 的 API 请求按全池热度排序选 key，不再长期固定 user/token primary key。
- 支持 Hikari 自有 routing key：`X-Hikari-Routing-Key` 只用于本地 hash 亲和，转发上游前必须剥离。
- 保持 Tavily adapter 兼容：`X-Project-ID` 仍可作为 routing subject 输入，并继续原样透传给 Tavily 上游。
- 保留 `GET /api/tavily/research/:request_id` 的 request-id key affinity 优先级。

### Non-goals

- 不增加跨 key 自动重试；上游不可知模式不能假设请求幂等。
- 不移除 `/api/tavily/*` façade、Tavily 请求体兼容或现有 token 配额/计费模型。
- 不触达生产 Tavily endpoint；验证限定本地或 mock upstream。
- 不改 UI，除非新增 effect code 无法被现有 fallback 正确展示。

## 范围（Scope）

### In scope

- 通用 API selector、backoff scope 与 request-log effect code。
- `X-Hikari-Routing-Key` 解析、hash、剥离与本地亲和。
- `X-Project-ID` 到通用 routing subject 的兼容映射。
- Rust 回归测试与 spec 索引。

### Out of scope

- 前端页面改版、Storybook 视觉证据。
- 生产 rollout、真实 Tavily endpoint smoke。
- 新增配置开关；本能力作为 `/api/tavily/*` 默认选路行为上线。

## 需求（Requirements）

### MUST

- API selector 的候选集合必须来自当前可用 key 池，排序优先级为：active `api_rebalance_http` cooldown、最近 60 秒上游 429 次数、最近 60 秒 billable/request 压力、`last_used_at` LRU、stable rank。
- 无 routing key 的 `/api/tavily/search|extract|crawl|map|research` 必须使用通用 API selector，而不是 user/token primary affinity。
- `X-Hikari-Routing-Key` 必须 trim 后本地 hash；空值视为不存在；原始值不得写入数据库或 request log。
- `X-Hikari-Routing-Key` 必须在转发上游前剥离。
- `X-Project-ID` 可作为 Tavily adapter 的 routing key fallback；它仍必须按原始 header 透传给上游。
- request-id affinity 必须优先于通用 API selector，用于 `/api/tavily/research/:request_id`。

### SHOULD

- 同 owner + 同 routing subject 在 key 健康时优先复用已绑定 key。
- 绑定 key 冷却或不可用时，必须在 stable pool 内重选更冷 key 并更新绑定。
- request log 应记录通用 binding / selection effect，便于 Admin 请求详情诊断。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 无 routing key：
  - 每次 API 请求通过 full-pool selector 选 key。
  - 上一次 429/cooldown 或近期压力会影响后续请求选路。
- 有 `X-Hikari-Routing-Key`：
  - owner subject 使用 `user:{user_id}`，无 user 时使用 `token:{token_id}`。
  - routing subject 使用 `sha256(trimmed_header_value)`。
  - 绑定仅保存 hash 与 owner subject，不保存原始 header 值。
- Tavily `X-Project-ID` 兼容：
  - 当 `X-Hikari-Routing-Key` 不存在且 `X-Project-ID` 非空时，用 `X-Project-ID` 作为 routing subject 输入。
  - `X-Project-ID` 不被剥离，继续发给 Tavily upstream。

### Edge cases / errors

- 所有候选都在 cooldown 时，仍选择排序后“最不差”的可用 key，而不是直接失败。
- 上游返回 429 或可临时 backoff 的 403 时，写入 `api_rebalance_http` scope，影响后续 API selector。
- selector 不做同请求自动 retry；当前请求按上游响应返回。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                   | 类型（Kind）  | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers）       | 备注（Notes）                                 |
| ------------------------------ | ------------- | ------------- | -------------- | ------------------------ | --------------- | ------------------------- | --------------------------------------------- |
| `X-Hikari-Routing-Key`         | HTTP header   | external      | New            | None                     | backend         | API clients               | 可选；本地使用后剥离，不透传上游              |
| `X-Project-ID` adapter mapping | HTTP header   | external      | Modify         | None                     | backend         | Tavily-compatible clients | 继续透传，同时可作为 routing subject fallback |
| `api_rebalance_http`           | backoff scope | internal      | New            | None                     | backend         | API selector              | 通用 API selector 的 transient backoff scope  |

### 契约文档（按 Kind 拆分）

- None

## 验收标准（Acceptance Criteria）

- Given 多个 healthy upstream keys 且 API 请求无 routing key
  When 连续调用 `/api/tavily/search`
  Then 请求不应长期固定 user/token primary key，并应按 LRU/热度分散到可用 key。

- Given 某 API 请求命中 key A 且上游返回 429
  When 后续无 routing key API 请求到达
  Then selector 应避开 key A 的 active `api_rebalance_http` cooldown。

- Given 请求带 `X-Hikari-Routing-Key`
  When mock upstream 收到转发请求
  Then 不应看到 `X-Hikari-Routing-Key`，且同 owner + 同 routing key 可复用绑定。

- Given 请求只带 `X-Project-ID`
  When 请求被代理到 Tavily upstream
  Then `X-Project-ID` 仍透传，同时本地可用其 hash 做 routing subject。

- Given 已创建 research request-id affinity
  When 调用 `/api/tavily/research/:request_id`
  Then 必须继续命中记录的 key。

## 验收清单（Acceptance checklist）

- [x] 核心路径的长期行为已被明确描述。
- [x] 关键边界/错误场景已被覆盖。
- [x] 涉及的接口/契约已写清楚或明确为 `None`。
- [x] 相关验收条件已经可以用于实现与 review 对齐。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit/integration tests: generic selector ordering, no-routing-key rebalance, routing-key stripping, `X-Project-ID` compatibility, research request-id priority.
- E2E tests: not required for this non-UI backend change.

### UI / Storybook (if applicable)

- Not applicable.

### Quality checks

- `cargo fmt --check`
- targeted Rust tests for API rebalance and Tavily HTTP proxy
- `cargo clippy --all-targets --all-features -- -D warnings`

## Visual Evidence

None

## Related PRs

- None

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：默认 API 选路从 primary affinity 改为 full-pool 后，依赖“同 token 总是同 key”的隐式客户端会观察到 key 漂移；有状态需求应改用 `X-Hikari-Routing-Key`。
- 假设：现有 Admin UI 对未知 effect code 有 fallback 展示，不需要本次改 UI。

## 参考（References）

- `../xm3dh-rebalance-mcp-gateway/SPEC.md`
- `../m30lm-http-project-affinity-x-project-id/SPEC.md`
