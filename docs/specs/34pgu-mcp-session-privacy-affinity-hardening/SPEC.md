# MCP 隐私收敛与 User/Token/Session 强亲和重构（#34pgu）

## 状态

- Status: 进行中（快车道）
- Created: 2026-03-27
- Last: 2026-03-27

## 背景 / 问题陈述

- 现有上游 key 选路同时依赖 `user_api_key_bindings` 的最近成功列表与 `token -> key` 的 15 分钟内存 TTL 亲和。这对普通 HTTP 请求足够，但对长生命周期 MCP 会话不稳定。
- MCP 现在已透传 `mcp-session-id` / `mcp-protocol-version` / `last-event-id`，如果后续请求在代理层重新调度到另一把 key，上游会看到“旧 session handle + 新 key”的不一致组合。
- `/mcp` 仍会向上游继续透传 `user-agent`、`accept-language`、`sec-ch-ua*`、`origin`、`referer` 等高指纹 header，暴露下游客户端环境特征。
- 代理当前直接把 upstream `mcp-session-id` 返回给客户端，意味着上游 session handle 会暴露给下游，也无法在代理侧强制 session principal / key pinning。

## 目标 / 非目标

### Goals

- 将 `user` 与其全部 `token` 收敛为持久单一 primary upstream key，移除 token 15 分钟软亲和作为主选路依据。
- 为 `/mcp` 引入代理 session registry，把对外 `mcp-session-id` 改为代理生成的 opaque session id，并强制 follow-up 请求命中 session 绑定 key。
- 在绑定 key 永久不可用时，自动原子换绑 `user + 所有 token`，同时吊销该 user 的 MCP sessions，要求客户端重新 `initialize`。
- `/mcp` 默认只透传协议必需头与少量通用语义头，统一固定 `user-agent`，丢弃高指纹 header。
- 通过 stable patch release + 101 rollout 完成上线与验收。

### Non-goals

- 不改普通 `/api/tavily/*` 的业务配额模型。
- 不重做完整 SSE replay broker；`last-event-id` 仅在同一 session + key 链路原样透传。
- 不改前端 UI、管理页展示或历史统计回填。
- 不允许继续以 silent round-robin / recent fallback 的方式在主路径漂移 user/token 绑定。

## 范围（Scope）

### In scope

- `docs/specs/README.md`
  - 新增 `34pgu-mcp-session-privacy-affinity-hardening` 索引，并在实现 / PR / merge / release / 101 验收后同步状态。
- `src/store/mod.rs`
  - 新增 `user_primary_api_key_affinity`、`token_primary_api_key_affinity`、`mcp_sessions` 持久化表与访问接口。
  - 提供 user/token 双层强亲和读写、原子换绑、session 创建/查询/失效接口。
- `src/models.rs`
  - 扩展 proxy request/response 与 session affinity 相关结构。
- `src/tavily_proxy/mod.rs`
  - 移除 token TTL 软亲和作为主调度依据，改为持久 primary affinity。
  - 实现自动换绑与 MCP 路径 header 隐私归一化。
- `src/server/proxy.rs`
  - 在 `/mcp` 路径上接管 opaque session id 映射、follow-up 反查、跨 token 拒绝和重连错误返回。
- `src/tests/mod.rs` / `src/server/tests.rs`
  - 覆盖强亲和、自动换绑、opaque session id、跨 token session 拒绝与 `/mcp` 指纹头归一化回归。
- `101` 部署资产
  - stable patch release 后更新 `/home/ivan/srv/ai/docker-compose.yml` 与 `/home/ivan/srv/ai/tavily-hikari.md` 的 immutable digest，并记录维护说明。

### Out of scope

- 普通 `/api/tavily/*` 的 header 策略调整。
- 生产直连 Tavily 测试。
- 浏览器 UI 改动与 Storybook 产物。

## 需求（Requirements）

### MUST

- 每个 `user_id` 仅允许一条 primary upstream key 绑定。
- 每个 `token_id` 仅允许一条 primary upstream key 绑定；若 token 绑定到某 user，则 token primary 必须与 user primary 完全一致。
- `/mcp initialize` 返回的 `mcp-session-id` 必须是代理生成的 opaque id，不能暴露 upstream session id。
- 同一个 opaque session 的后续请求必须固定命中同一把 upstream key，不能跨 key 漂移。
- `/mcp` 必须丢弃 `accept-language`、`sec-ch-ua*`、`origin`、`referer` 与真实 UA 指纹，统一使用代理 UA。
- 当绑定 key 永久不可用（删除、禁用、隔离、不可继续复用）时，自动换绑 `user + 全部 tokens`，并使旧 MCP sessions 失效。

### SHOULD

- 对仍未建立 primary affinity 的既有用户，优先从 legacy `user_api_key_bindings` 的最近成功 key 做一次性回填；若无法安全回填，则在首次请求时惰性建立 primary affinity。
- request/admin logs 不记录 raw upstream session id。

## 功能与行为规格（Functional/Behavior Spec）

### Strong affinity

- 新请求若带 `auth_token_id`：
  - 若 token 绑定了 user，则优先解析 `user_primary_api_key_affinity` 与 `token_primary_api_key_affinity`，并强制收敛到同一 key。
  - 若 user/token 尚未建立 primary affinity，则只允许选一把 key 后持久写入，而不是留在软亲和缓存里。
  - 若绑定 key 不可用，代理必须自动换绑并同步该 user 下全部 tokens。
- 未绑定 user 的 token 仍需拥有持久 `token_primary_api_key_affinity`，而不是 15 分钟 TTL。

### MCP opaque session mapping

- `initialize` 成功且上游返回 `mcp-session-id` 时：
  - 代理生成新的 opaque `proxy_session_id`。
  - 本地保存 `proxy_session_id -> upstream_session_id + upstream_key_id + auth_token_id + user_id + protocol_version`。
  - 返回给客户端的只能是 `proxy_session_id`。
- 客户端 follow-up 请求带回 `proxy_session_id` 时：
  - 代理必须先反查本地 session，再把 header 改写为 upstream session id，且强制请求走 session 绑定 key。
  - 若当前 token 不是该 session 的 owner，代理本地拒绝。
  - 若 session 已失效、已过期或 key 已换绑，代理返回“需重连”的本地错误，不向上游继续透传旧 session。

### MCP header privacy

- `/mcp` 只保留：
  - `accept`
  - `accept-encoding`
  - `cache-control`
  - `content-type`
  - `last-event-id`
  - `mcp-protocol-version`
  - `mcp-session-id`
  - `pragma`
  - `x-mcp-*`
  - `x-tavily-*`
  - `tavily-*`
- `/mcp` 一律固定 `user-agent` 为代理标识，不透传客户端原始 UA。
- `/mcp` 丢弃 `accept-language`、`origin`、`referer`、`sec-ch-ua*`、`sec-fetch-*` 等高指纹头。

## 验收标准（Acceptance Criteria）

- Given 某 user 已有两个 token
  When 它们分别发起代理请求
  Then 两个 token 都命中同一把 upstream key，且重启后仍保持一致。
- Given 某 token 已有 primary key 绑定
  When 超过原先 15 分钟软亲和窗口或进程重启
  Then 后续请求仍命中同一把 primary key，而不是重新调度。
- Given 某 `/mcp initialize` 请求成功
  When 客户端读取响应头
  Then 只能看到代理生成的 opaque session id，而不是 upstream session id。
- Given 某 opaque session 已建立
  When 客户端继续发送 `notifications/initialized` / `tools/list`
  Then 请求必须命中同一 upstream key，且上游收到的是 upstream session id 而不是 opaque session id。
- Given `--dev-open-admin` 启用且客户端未显式提供 token
  When 客户端请求 `/mcp` 或 `/mcp/*`
  Then 代理必须本地返回 `401 explicit_token_required`，而不是复用共享 fallback principal。
- Given 另一个 token 试图复用同一个 opaque session
  When 请求到达代理
  Then 代理本地拒绝，不向上游转发。
- Given 绑定 key 被禁用 / 隔离 / 删除
  When 该 user 的下一次请求到达
  Then 代理自动换绑 user + 全部 tokens，并使旧 MCP sessions 失效、要求客户端重连。
- Given 某 MCP session 已固定到一把 upstream key
  When 该 key 仅变为 `exhausted` 且未被禁用 / 隔离 / 删除
  Then 代理必须继续沿用同一把 key，而不是强制断开该 session。
- Given `/mcp` 请求带 `accept-language`、`sec-ch-ua`、`origin`、`referer`
  When 请求经代理转发
  Then 上游收不到这些头，且收到的是统一代理 `user-agent`。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: `cargo test primary_api_key_affinity`
- Integration tests: `cargo test mcp_session_`
- Full regression for branch: `cargo test`

### Quality checks

- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- GitHub CI 全绿
- stable patch release 成功

## 文档更新（Docs to Update）

- `docs/specs/README.md`
- `/home/ivan/srv/ai/tavily-hikari.md`
- `/home/ivan/srv/maintenance/<date>-ops-ai-tavily-hikari-mcp-affinity-privacy-<version>.md`

## 计划资产（Plan assets）

- Directory: `docs/specs/34pgu-mcp-session-privacy-affinity-hardening/assets/`
- Visual evidence source: None（非 UI 交付面）

## Visual Evidence

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 建立 spec 与 README 索引，锁定快车道发布 / 101 验收口径
- [x] M2: user/token 双层强亲和持久化与自动换绑落地
- [x] M3: `/mcp` opaque session 映射与 strict privacy header 落地
- [x] M4: 回归测试、本地质量门、review-loop、PR 收敛通过
- [ ] M5: stable patch release、101 rollout、线上验收与 cleanup 完成

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：既有历史 user 可能存在多把 legacy recent keys；本实现默认优先选择最近一次成功且当前仍可用的 key 进行回填，剩余未回填用户在首次请求时惰性建立 primary affinity。
- 风险：若所有可选 key 都已不可继续复用，自动换绑会失败并返回无可用 key，而不是静默漂移到旧 session 的另一把 key。
- 开放问题：None
- 假设：`auth_token_id` 仍是 MCP session principal 的唯一判定基准；`user_id` 仅用于同步换绑与审计。

## 变更记录（Change log）

- 2026-03-27: 新建 spec，锁定 user/token/session 强亲和、opaque session 与 `/mcp` strict privacy header 范围。
- 2026-03-27: 完成 user/token 持久 primary affinity、MCP opaque session registry、`/mcp` strict header sanitizer、dev-open-admin 显式 token 约束，以及 disabled/quarantined/exhausted 场景的 session/rebind 回归覆盖。
- 2026-03-27: 创建 PR #189，进入快车道 PR 收敛 / stable patch / 101 rollout 阶段。

## 参考（References）

- `src/store/mod.rs`
- `src/tavily_proxy/mod.rs`
- `src/server/proxy.rs`
- `src/tests/mod.rs`
- `src/server/tests.rs`
