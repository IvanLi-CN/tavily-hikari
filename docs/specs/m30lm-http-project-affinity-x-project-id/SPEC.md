# HTTP `X-Project-ID` 项目亲和与热度避让（#m30lm）

## 状态

- Status: 已完成（merge-ready）
- Created: 2026-04-10
- Last: 2026-04-10

## 背景 / 问题陈述

- `/api/tavily/*` 当前仅按 `user/token primary affinity` 选 upstream key；同一 `X-Project-ID` 的请求可能在不同 key 之间漂移。
- MCP 已经具备稳定池内排序、429 cooldown 与共享热度规避，但 HTTP 侧没有对应的项目级亲和能力。
- `X-Project-ID` 已作为 Tavily 官方 project tracking header 透传上游；若代理侧不收敛项目级 key 选路，则同一项目的热度状态与问题定位都不稳定。
- `GET /api/tavily/research/:request_id` 已经依赖 `request_id -> key_id` 亲和；新方案必须保留该优先级，不能让项目亲和覆盖 research 结果查询的既有 pinning。

## 目标 / 非目标

### Goals

- 为 `/api/tavily/search|extract|crawl|map|research` 引入 owner-scoped `X-Project-ID` 项目亲和。
- 将同一 owner 下相同 `X-Project-ID` 的请求优先放入同一 stable top-N upstream key 池，默认复用既有项目绑定。
- 复用 MCP 的热 key 规避思想，为 HTTP 项目亲和新增独立 `http_project_affinity` transient backoff scope。
- 保持 `research request_id affinity` 的更高优先级：`request_id` > project affinity > existing user/token primary affinity > global。
- 保持 `X-Project-ID` 原样向上游透传，仅在本地使用其 hash 做亲和与持久化。

### Non-goals

- 不修改 `X-Project-ID` 的 wire shape，不伪造、不改写、不移除该 header。
- 不在本次改动里为 HTTP API 增加 `Retry-After` 自动等待与重试。
- 不同时推进 HTTP 其他 header 指纹收敛。
- 不改变 `/api/tavily/usage` 的本地统计接口语义；该接口不参与 upstream key 选路。

## 范围（Scope）

### In scope

- `docs/specs/README.md`
  - 新增 `m30lm-http-project-affinity-x-project-id` 索引，并在 PR / merge-ready 收口后同步状态。
- `src/models.rs`
  - 增补 HTTP project affinity 上下文 / 绑定结构。
- `src/store/mod.rs`
  - 新增项目绑定持久化表与访问接口。
  - 复用 `api_key_transient_backoffs`，但新增 `http_project_affinity` scope。
- `src/tavily_proxy/mod.rs`
  - 实现项目级 stable pool 排序、优先复用、cooldown 规避、热度避让与重绑逻辑。
  - 在 HTTP 代理路径记录新的 key effect code，并为 HTTP 429 arm `http_project_affinity` cooldown。
- `src/server/handlers/tavily.rs`
  - 解析 `X-Project-ID`，在非 `dev-open-admin` fallback 场景下接入 HTTP 项目亲和。
  - `GET /api/tavily/research/:request_id` 继续先走 `request_id` 亲和。
- `src/tests/mod.rs` / `src/server/tests.rs`
  - 覆盖项目亲和复用、同名项目跨 owner 隔离、429 后项目避热重绑、research create/result 优先级与 raw header 透传。

### Out of scope

- MCP session routing、session registry 与 MCP header privacy。
- UI / Storybook 改动。
- 生产 Tavily upstream 测试。

## 需求（Requirements）

### MUST

- 当 HTTP 请求带非空 `X-Project-ID` 时，代理必须将其 `trim` 后作为本地项目亲和输入。
- 本地项目 subject 必须按以下优先级生成：
  - `user:{user_id}:project:{sha256(trimmed_project_id)}`
  - 无 user 绑定时退化为 `token:{token_id}:project:{sha256(trimmed_project_id)}`
- `--dev-open-admin` fallback（无显式稳定 token owner）必须禁用项目亲和，回落现有逻辑。
- 本地数据库不得存 raw `X-Project-ID`，只能存 hash / owner-scoped subject。
- `GET /api/tavily/research/:request_id` 必须继续优先命中 `request_id` 绑定 key，即使请求头带 `X-Project-ID` 也不能改写该优先级。
- HTTP 项目亲和候选排序必须固定为：
  1. active `http_project_affinity` cooldown
  2. 最近 60s upstream `429` 次数
  3. 最近 60s billable 请求压力
  4. `last_used_at` LRU
  5. stable rank
- 绑定中的项目 key 只要仍可用且未命中 HTTP-project cooldown，就必须优先复用。
- 当绑定 key 不可用或处于项目 cooldown 时，代理必须在同一 stable pool 内重选更冷 key，并更新持久绑定。

### SHOULD

- 项目首次绑定时，若 stable pool 的首选 key 未被避让，应记录明确的 `bound` key effect。
- 因 key disabled / quarantined / deleted 等不可复用原因触发的项目重绑，应记录明确的 `rebound` key effect。
- 因 HTTP 429 cooldown / 最近 429 热度 / 最近 billable 压力导致的新选路，应暴露专门的 avoided key effect，便于 request log 追踪。

## 功能与行为规格（Functional / Behavior Spec）

### Project affinity subject & persistence

- 请求头解析：
  - `X-Project-ID` 仅做 `trim` 与空字符串过滤。
  - 空值视为“未提供”，不触发项目亲和。
- 本地持久化：
  - 新表按 `owner_subject + project_hash` 唯一键保存 `api_key_id`、`created_at`、`updated_at`。
  - `owner_subject` 固定为 `user:{user_id}` 或 `token:{token_id}`。
  - `project_hash` 固定为 `sha256(trimmed_project_id)` 的 hex 编码。

### HTTP 选路优先级

- `GET /api/tavily/research/:request_id`
  - 继续使用 `request_id -> key_id` 亲和；项目亲和不参与此路径选路。
- 其余 `/api/tavily/*` 上游代理请求
  - 若可解析项目亲和 subject，则优先走 HTTP project affinity。
  - 若项目亲和不可用（缺 header / 空 header / dev fallback / 无 token id），回退到既有 `user/token primary affinity`。
  - 若无 token id，则继续使用全局 key 调度。

### Stable pool 与热度避让

- stable pool 的候选 key 集合与 MCP 一样仅从当前可用 key 集合中构造，不允许越池漂移。
- 排序时仅考虑 HTTP 项目亲和相关信号：
  - `http_project_affinity` scope 的 active cooldown
  - 最近 60 秒 `failure_kind = upstream_rate_limited_429`
  - 最近 60 秒 billable 请求计数
  - `api_keys.last_used_at`
  - stable rank
- 若已绑定项目 key 不在 cooldown 且仍可 `try_acquire_specific_key`，直接复用，不再重排。
- 若绑定 key 在 cooldown，但 stable pool 内存在更好的候选，则必须改走更冷 key 并更新绑定。
- 若所有 stable pool 候选都处于 cooldown，仍需选择“最不差”的 key，而不是返回 `NoAvailableKeys`。

### HTTP 项目级 cooldown

- 当使用项目亲和选出的 HTTP 请求收到 upstream `429` 时：
  - 解析 `Retry-After`
  - 以 `http_project_affinity` 作为 scope arm 对应 key 的 transient backoff
  - 仅影响未来项目亲和的新请求，不影响既有 MCP session 绑定，也不对当前 HTTP 请求自动重试
- 当 HTTP 请求未启用项目亲和（缺失/空白 `X-Project-ID` 等）且收到 upstream `429` 时：
  - 必须保留当前既有 `mcp_session_init` backoff 写入语义，避免普通 HTTP 流量的 rate-limit 信号回归丢失
- `GET /api/tavily/research/:request_id` 作为非项目亲和路径的一部分，也必须继续保留既有 `mcp_session_init` cooldown 写入与 request-log 关联语义
- request log 需要继续通过 `source_request_log_id` 关联该 backoff 行，便于审计。

### Request log key effects

- HTTP 项目亲和新增 / 扩展以下 key effect code：
  - `http_project_affinity_reused`
  - `http_project_affinity_bound`
  - `http_project_affinity_rebound`
  - `http_project_affinity_cooldown_avoided`
  - `http_project_affinity_rate_limit_avoided`
  - `http_project_affinity_pressure_avoided`
- 这些 code 只用于说明“项目亲和为何选到这把 key”；HTTP 当前请求如果还触发 key quarantine / exhausted 等更高优先级健康动作，仍以健康动作的 key effect 为准。

## 验收标准（Acceptance Criteria）

- Given 同一 user 下连续两次 `POST /api/tavily/search`，且两次都带相同非空 `X-Project-ID`
  When 项目绑定 key 仍健康且不在 cooldown
  Then 两次请求必须命中同一 upstream key。
- Given 同一 user 下两个不同 `X-Project-ID`
  When 它们分别建立项目绑定
  Then 不能共享同一条持久项目绑定记录。
- Given 两个不同 user 使用相同的 `X-Project-ID` 文本
  When 它们分别建立项目绑定
  Then 不能复用同一条本地项目绑定记录。
- Given 某项目绑定 key 因 upstream `429` 被写入 `http_project_affinity` cooldown
  When 后续同项目新请求到达，且 stable pool 内存在更冷 key
  Then 代理必须优先改走更冷 key，并更新项目绑定。
- Given `POST /api/tavily/research` 带 `X-Project-ID`
  When research create 成功返回 `request_id`
  Then 后续 `GET /api/tavily/research/:request_id` 即使不带 `X-Project-ID`，仍必须命中原 research key。
- Given 请求缺失或传入空白 `X-Project-ID`
  When 请求到达 `/api/tavily/*`
  Then 代理必须完全回退到当前 `user/token primary affinity` / global 行为。
- Given `--dev-open-admin` fallback 但请求未显式提供稳定 token
  When 请求到达 `/api/tavily/*`
  Then 不得启用项目亲和。
- Given 请求头带 `X-Project-ID`
  When 请求被转发到上游
  Then 上游仍能收到原始 `X-Project-ID`。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- `cargo test http_project_affinity`
- `cargo test tavily_http_research_result`
- `cargo test tavily_http_search`
- `cargo test`

### Quality checks

- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- GitHub CI 全绿
- PR 达到 merge-ready

## 文档更新（Docs to Update）

- `docs/specs/README.md`

## 计划资产（Plan assets）

- Directory: `docs/specs/m30lm-http-project-affinity-x-project-id/assets/`
- Visual evidence source: None（非 UI 交付面）

## Visual Evidence

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 建立 spec 与 README 索引，冻结 owner-scoped project subject、优先级与 key effect 合同
- [x] M2: 持久化项目绑定与 `http_project_affinity` backoff scope 落地
- [x] M3: HTTP 代理路径接入项目亲和与 research 优先级保护
- [x] M4: 回归测试、本地质量门、review-loop、PR 收敛通过
- [x] M5: PR 达到 merge-ready（不自动 merge / cleanup）

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：`X-Project-ID` 的高基数输入可能导致项目绑定表增长较快；当前假设其基数可接受，后续如需 TTL / GC 再单独演进。
- 风险：HTTP 项目亲和沿用当前 stable pool key 数量设置，未新增独立 project-affinity pool 配置。
- 开放问题：None
- 假设：`request_logs` 的最近 60 秒 billable / 429 热度统计足以代表项目级避热信号，无需新增专门计数表。

## 变更记录（Change log）

- 2026-04-10: 新建 spec，锁定 owner-scoped `X-Project-ID` 项目亲和、独立 HTTP backoff scope、request-id 优先级与 request log key effects。
- 2026-04-10: PR #227 收口为 merge-ready，补齐 full-target clippy 修复、项目亲和回归覆盖与 PR release labels。
- 2026-04-10: review follow-up 补充无项目亲和 HTTP `429` 继续写入既有 `mcp_session_init` cooldown，避免默认 HTTP 路径回归。
- 2026-04-10: review follow-up 补充 research result GET `429` 继续写入既有 `mcp_session_init` cooldown 与 request-log 关联，避免轮询路径回归。

## 参考（References）

- `src/store/mod.rs`
- `src/tavily_proxy/mod.rs`
- `src/server/handlers/tavily.rs`
- `src/tests/mod.rs`
- `src/server/tests.rs`
