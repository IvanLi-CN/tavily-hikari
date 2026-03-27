# MCP 会话头透传热修直达 101 验收（#uuhup）

## 状态

- Status: 已完成（快车道）
- Created: 2026-03-27
- Last: 2026-03-27

## 背景 / 问题陈述

- 生产 `/mcp` 请求在 `initialize` 之后的后续会话调用大量失败，管理页与线上日志都集中出现 `Missing mcp-session-id header. Please reconnect to start a new session.`。
- 现有共享 header sanitize 逻辑只放行 `x-mcp-*` 前缀，未放行标准 MCP Streamable HTTP 会话头 `mcp-session-id`、`mcp-protocol-version`，因此代理在转发到上游前把这些头错误丢弃。
- `/api/tavily/*` 常规 HTTP API 仍然大体正常，说明故障范围集中在 MCP stateful session 链路；若不修复，会持续影响 MCP 初始化后的发现与工具调用。

## 目标 / 非目标

### Goals

- 以最小热修恢复 MCP 会话调用，明确放行 `mcp-session-id`、`mcp-protocol-version` 与兼容恢复所需的 `last-event-id`。
- 保持既有代理链路/来源暴露头阻断策略不变，不扩大为全量 header 策略重写。
- 补齐单元与集成回归，覆盖会话头透传、代理来源头继续丢弃，以及 `initialize -> sessionful MCP call` 成功链路。
- 通过 stable patch release 产出新 immutable digest，并将 101 `ai-tavily-hikari` 更新到新 digest 后完成线上验收。

### Non-goals

- 不回填、重写或重分类历史 `request_logs` / `auth_token_logs`。
- 不调整前端 UI、Storybook、管理页统计口径或 Tavily 业务计费逻辑。
- 不对生产直连 Tavily 做真实写流量测试；所有本地测试都使用 mock/stub upstream。

## 范围（Scope）

### In scope

- `docs/specs/README.md`
  - 新增 `uuhup-mcp-session-header-forwarding-hotfix` 索引，并在交付过程中同步状态/备注。
- `src/lib.rs` / `src/analysis.rs`
  - 扩展共享允许头集合，精确放行 `mcp-session-id`、`mcp-protocol-version`、`last-event-id`。
- `src/tests/mod.rs`
  - 新增 sanitize 单元测试，锁住上述会话头透传与 `x-forwarded-*` / `x-real-ip` 继续丢弃。
- `src/server/tests.rs`
  - 新增 mock upstream 会话集成测试，验证 `initialize` 返回的 session header 能被客户端收到，且后续带 `mcp-session-id` 的 MCP 请求经代理后成功到达上游。
- 101 `ai` stack 部署资料
  - stable release 后更新 `/home/ivan/srv/ai/docker-compose.yml` 与 `/home/ivan/srv/ai/tavily-hikari.md` 到新 digest。
  - 在 `/home/ivan/srv/maintenance/` 记录本次 hotfix 的发布、验证与回滚 digest。

### Out of scope

- 修改响应头透传实现。
- 扩展到其它未在事故证据中出现的协议头审计或 `/mcp/*` 子路径语义调整。
- 调整 release workflow、label gate 或 101 的部署流程本身。

## 需求（Requirements）

### MUST

- 共享 sanitize 逻辑必须保留 `mcp-session-id`、`mcp-protocol-version`、`last-event-id`。
- `forwarded`、`x-forwarded-*`、`x-real-ip`、`cdn-loop` 等现有来源暴露头必须继续被丢弃。
- 本地回归必须只使用 mock/stub upstream，不得触达生产 Tavily。
- PR 必须按 stable patch 热修路径发布，101 必须切换到 release workflow 产出的 immutable digest。

### SHOULD

- 集成测试尽量直接覆盖“收到 session header -> 继续请求成功”的用户侧核心链路。
- spec 与 README 索引在 PR、merge、release、101 验收后保持同步，无漂移。

### COULD

- 若实现中顺手发现 `last-event-id` 尚未被任何测试覆盖，可用最小单元测试补齐，不额外引入完整 SSE 重放框架。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 客户端向 `/mcp` 发送 `initialize`，若上游返回 `Mcp-Session-Id`，代理必须原样返回该响应头给客户端。
- 客户端后续向 `/mcp` 发送 `notifications/initialized`、`tools/list`、`prompts/list`、`resources/list`、`tools/call` 等会话相关请求时，只要携带 `mcp-session-id` / `mcp-protocol-version`，代理必须原样转发到上游。
- `last-event-id` 若由客户端带入恢复请求，代理必须保留该头，便于上游按事件流恢复语义处理。

### Edge cases / errors

- 若客户端没有携带 `mcp-session-id`，代理不做本地补造，继续保持现有透明转发行为，由上游决定返回错误。
- 若请求包含 `x-forwarded-*`、`x-real-ip`、`forwarded`、`cdn-loop` 等来源相关头，代理仍必须丢弃，避免泄漏真实链路信息。
- 非 MCP HTTP API 的 header 行为不应被本热修改变。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                  | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）                  |
| ----------------------------- | ------------ | ------------- | -------------- | ------------------------ | --------------- | ------------------- | ------------------------------ |
| MCP request header forwarding | internal     | internal      | Modify         | None                     | backend         | `/mcp` proxy path   | 只变更 allowlist，不新增新接口 |

### 契约文档（按 Kind 拆分）

None

## 验收标准（Acceptance Criteria）

- Given 客户端向 `/mcp` 发起 `initialize`
  When 上游响应中带 `Mcp-Session-Id`
  Then 客户端能从代理响应头读到相同的 session id。
- Given 客户端已收到 `Mcp-Session-Id`
  When 客户端继续发送带 `mcp-session-id` 与 `mcp-protocol-version` 的 `notifications/initialized` 或 `tools/list`
  Then 代理把这些头透传给上游，且请求不再因缺失 session header 被上游拒绝。
- Given 客户端请求里带有 `x-forwarded-for`、`x-forwarded-host` 或 `x-real-ip`
  When 请求经共享 sanitize 流程转发
  Then 上游侧仍收不到这些来源暴露头。
- Given stable release workflow 成功发布新 digest
  When 101 `ai-tavily-hikari` 更新到该 digest
  Then 容器健康、`/api/version` 返回新版本，且部署后成功样本明确显示 `mcp-session-id` / `mcp-protocol-version` 已透传；若仍出现 `Missing mcp-session-id header`，必须能证明是客户端请求本身未携带该头，而不是代理继续丢头。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: `cargo test sanitize_headers_`
- Integration tests: `cargo test mcp_`
- Full regression for hotfix branch: `cargo test`

### Quality checks

- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- GitHub CI 全绿
- stable release workflow 成功

## 文档更新（Docs to Update）

- `docs/specs/README.md`: 追加 spec 索引，并在交付后同步 `Status` / `Last` / `Notes`
- `/home/ivan/srv/ai/tavily-hikari.md`: 更新生产 digest 与 hotfix 描述
- `/home/ivan/srv/maintenance/<date>-ops-ai-tavily-hikari-mcp-session-header-hotfix-<version>.md`: 记录上线与回滚信息

## 计划资产（Plan assets）

- Directory: `docs/specs/uuhup-mcp-session-header-forwarding-hotfix/assets/`
- Visual evidence source: None（本次变更不涉及 UI 交付面）

## Visual Evidence

None

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 锁定热修 spec、release 标签要求与 101 上线/回滚口径
- [x] M2: 放行会话恢复所需 MCP 头，保持来源暴露头阻断不变
- [x] M3: 补齐单元/集成回归并通过本地质量门
- [x] M4: 完成 PR、合并、stable release、101 部署与线上验收

## 方案概述（Approach, high-level）

- 在共享 header allowlist 上做最小增量修改，而不是按路径引入新的特判逻辑。
- 用一个直接覆盖用户核心问题的 mock upstream 集成测试证明修复有效：先返回 session header，再要求后续请求带回该 header。
- 发布继续遵循既有 stable patch 流程与 101 手动同步 digest 的部署卡约定。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：若上游还新增了其它 mandatory MCP 头，本热修只解决事故证据里已确认的会话恢复头，不做无限扩张。
- 风险：若 release 后 101 未及时切换 immutable digest，线上仍会继续报旧错误。
- 开放问题：None
- 假设（需主人确认）：release 标签使用 `type:patch` + `channel:stable`，并继续手动同步 101 compose/card。

## 变更记录（Change log）

- 2026-03-27: 新建 hotfix spec，锁定 MCP 会话头透传、stable release 与 101 验收范围。
- 2026-03-27: 完成共享 allowlist 修改、单元/集成回归与本地质量门验证，等待 PR、release 与 101 收口。
- 2026-03-27: PR #184 合并后发布 stable `v0.29.5`，Release workflow `23640976259` 产出 GHCR digest `sha256:1b641d816609e432e012ce9ad8d1d090cbc95d8ee107f923c4646a35bfc7e162`。
- 2026-03-27: 101 `ai-tavily-hikari` 已更新到 `v0.29.5`；部署后 `initialize -> notifications/initialized -> tools/list -> prompts/list` 成功链路确认透传 `mcp-session-id` / `mcp-protocol-version`，残留 `Missing mcp-session-id header` 400 样本均为客户端未携带 session header，而非代理继续丢头。

## 参考（References）

- `src/lib.rs`
- `src/analysis.rs`
- `src/tests/mod.rs`
- `src/server/tests.rs`
- `/home/ivan/srv/ai/docker-compose.yml`
- `/home/ivan/srv/ai/tavily-hikari.md`
