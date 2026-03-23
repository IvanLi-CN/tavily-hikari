# MCP 子路径本地拒绝与上游阻断（#hrs2p）

## 状态

- Status: 已完成（快车道）
- Created: 2026-03-23
- Last: 2026-03-23

## 背景 / 问题陈述

- 线上历史请求里出现了 `/mcp/search`、`/mcp/extract`、`/mcp/v1/messages`、`/mcp/chat/completions` 这类非根路径 MCP 访问。
- 现有服务把 `/mcp` 与 `/mcp/*path` 都交给同一个 proxy handler，导致这些错误入口被原样转发到 Tavily MCP upstream，产生无意义的 `404 Not Found` 上游流量与 request logs。
- 这类请求大多来自协议或 endpoint 配置错误的客户端，本地服务应当明确拒绝，而不是继续把错误入口透传给上游。

## 目标 / 非目标

### Goals

- 仅保留根路径 `/mcp` 作为受支持的 MCP ingress。
- 将所有 `/mcp/*` 非根路径改为本地直接返回 plain-text `404 Not Found`，不再命中上游。
- 子路径拒绝继续沿用当前 token 解析与校验语义：缺 token / 无效 token 仍返回现有 `401`。
- 对已通过鉴权的子路径拒绝保留后台留痕：写入 `request_logs` 与 token logs，并显式标注 `mcp_path_404`。
- 子路径拒绝不得选择上游 key、不得计入 business quota、不得写入 credits、不得产生 key health side effect。

### Non-goals

- 不修复历史 `/mcp/*` request logs。
- 不修改 `/admin/requests` 现有 raw request-kind 筛选异常。
- 不改变根路径 `/mcp` 的 JSON-RPC/MCP 协议语义。
- 不改任何 `/api/tavily/*` HTTP API 路由或计费逻辑。

## 范围（Scope）

### In scope

- `docs/specs/README.md`
  - 新增 `hrs2p-mcp-subpath-local-rejection` 索引。
- `src/server/serve.rs`
  - 拆分 `/mcp` 与 `/mcp/*path` 路由。
- `src/server/proxy.rs`
  - 新增本地子路径拒绝 handler，复用 token 解析/校验逻辑，并保留根路径 `/mcp` 的现有行为。
- `src/models.rs`
  - 允许 request log 在没有上游 key 的前提下持久化。
- `src/store/mod.rs`
  - 放宽 `request_logs.api_key_id` 为可空，允许写入无 key 的 request log，并保证 rollup/读取路径兼容。
- `src/server/tests.rs`
  - 补齐子路径本地拒绝、无上游命中、鉴权兼容、日志持久化与根路径回归测试。

### Out of scope

- 历史 bad logs 的回补、清洗或标签重算。
- token detail / admin requests UI 的额外筛选或展示改造。
- 任何新的 MCP 子协议支持。

## 验收标准（Acceptance Criteria）

- Given 已鉴权请求访问 `/mcp/search`、`/mcp/extract`、`/mcp/v1/messages` 或 `/mcp/chat/completions`
  When 请求进入服务
  Then 服务直接返回本地 plain-text `404 Not Found`，且不会向上游发出任何请求。
- Given 缺少 token 或 token 无效的 `/mcp/*` 请求
  When 请求进入服务
  Then 响应仍保持当前 `401` 语义，不会退化成匿名 `404`。
- Given 已鉴权的 `/mcp/*` 本地拒绝
  When 日志落盘
  Then `request_logs` 与 `auth_token_logs` 都能看到该请求，`request_kind` 为 `mcp:raw:<full_path>`，`result_status=error`，`failure_kind=mcp_path_404`，`counts_business_quota=false`，`business_credits=NULL`，`api_key_id=NULL`，`key_effect_code=none`。
- Given 根路径 `/mcp` 的正常请求
  When 请求访问 `/mcp`
  Then 继续走现有 proxy/billing/日志逻辑，不受子路径拒绝改动影响。
- Given 根路径 `/mcp` 的 `GET + Accept: text/event-stream`
  When 请求进入服务
  Then 继续保持现有 `405 Method Not Allowed` 行为。
- Given 历史 legacy raw MCP kind（例如 `/mcp/sse`）的回填/筛选测试
  When 运行现有回归
  Then 仍保持通过，不因运行时路由改动而破坏历史解释逻辑。

## 非功能性验收 / 质量门槛（Quality Gates）

- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 冻结 `/mcp` 根路径与 `/mcp/*` 子路径的运行时契约和日志契约
- [x] M2: 路由拆分完成，子路径改为本地拒绝且不触发上游
- [x] M3: request log / token log 能记录无 key 的本地 `mcp_path_404` 拒绝
- [x] M4: 测试、review-loop、PR、merge 与 cleanup 收口完成

## 风险 / 假设

- 风险：`request_logs` 的 `api_key_id` 目前依赖较多，放宽为可空时需要同步检查 rollup 与读取路径，避免旧查询在遇到 `NULL` 时报错。
- 假设：子路径本地拒绝只保留 token 解析/校验语义，不继续参与 business quota 或上游 key 选择流程。
- 假设：本地 `404` 仍使用 plain-text `"Not Found"`，不额外包 JSON 错误对象。

## 参考（References）

- `src/server/serve.rs`
- `src/server/proxy.rs`
- `src/store/mod.rs`
- `src/tavily_proxy/mod.rs`
