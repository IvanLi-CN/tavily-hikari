# LinuxDo 登录入口与自动填充 Token（#rg5ju）

## 状态

- Status: 已完成
- Created: 2026-02-26
- Last: 2026-02-26

## 背景 / 问题陈述

- 当前首页只有管理员入口，普通用户无法通过 LinuxDo 直接登录并拿到自己的一对一 token。
- `auth_tokens` 已存在，但没有“用户身份 -> token”的绑定模型，导致用户态 token 获取依赖手工分发。
- 目标站点 `https://tavily.ivanli.cc/` 需要在首页实现一致且可理解的登录入口（区域①）和 token 自动填充（区域②）。

## 目标 / 非目标

### Goals

- 接入 LinuxDo Connect OAuth2 授权码流程，建立用户会话。
- 新增独立用户体系（与管理员体系隔离），实现用户与现有 `auth_tokens` 一对一绑定。
- 首页未登录时显示 Linux DO 登录按钮；已登录后隐藏该入口并自动填充 token。
- 保持旧行为：自动填充后写入 URL hash。

### Non-goals

- 不做 RBAC / 多角色权限系统。
- 不做用户侧 token 轮换、删除、自助重建。
- 不实现 LinuxDo 之外 provider 的真实接入（仅保留数据模型扩展能力）。

## 范围（Scope）

### In scope

- 后端 OAuth2 登录路由、会话路由、用户 token 读取路由。
- SQLite 新增用户/第三方绑定/会话/state/token 绑定表与升级逻辑。
- `GET /api/profile` 增加用户态字段（可选字段，向后兼容）。
- 首页区域①②交互、Linux DO 按钮样式、i18n 文案。
- 相关自动化测试与文档更新。

### Out of scope

- 改造管理员鉴权链路（ForwardAuth + builtin admin）。
- 真实 LinuxDo 线上稳定性和限流策略覆盖。
- 新的前端页面路由（仅在现有首页整合交互）。

## 需求（Requirements）

### MUST

- 必须使用 LinuxDo Connect OAuth2 端点（authorize/token/api/user）。
- 必须校验 OAuth `state` 且一次性消费，防重放与 CSRF。
- 必须为首次登录用户自动创建一个 `auth_tokens` 并建立一对一绑定。
- 必须保持“后续登录不新增 token”。
- 必须在首页实现：未登录显示 Linux DO 登录按钮；已登录隐藏区域①并自动填充区域② token。
- 必须保持 `GET /api/profile` 旧字段兼容。

### SHOULD

- 用户登录会话应持久化（非进程内存），支持重启后仍有效到期。
- 新增配置项应支持多环境回调地址和端点覆盖。

### COULD

- `oauth_accounts` 预留多 provider 字段与索引，便于后续扩展。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 用户点击首页 Linux DO 登录按钮，跳转 `/auth/linuxdo`，后端生成 state 并重定向到 LinuxDo authorize。
- LinuxDo 回调 `/auth/linuxdo/callback`，后端完成 code 换 token、拉用户信息、upsert 本地用户与 oauth 绑定、创建用户会话 cookie。
- 登录成功后首页加载 profile，若 `userLoggedIn=true` 则调用 `/api/user/token` 自动填充 token 输入框并写 hash。

### Edge cases / errors

- `state` 缺失、过期、重复、与会话不匹配时返回 4xx 并拒绝登录。
- token endpoint 或 userinfo endpoint 失败时返回 502/500 且不写本地会话。
- 若用户绑定 token 被删除/禁用，`/api/user/token` 返回可诊断错误（不自动重建）。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                 | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）                |
| ---------------------------- | ------------ | ------------- | -------------- | ------------------------ | --------------- | ------------------- | ---------------------------- |
| LinuxDo OAuth Start          | HTTP         | external      | New            | ./contracts/http-apis.md | Backend         | Browser             | `GET /auth/linuxdo`          |
| LinuxDo OAuth Callback       | HTTP         | external      | New            | ./contracts/http-apis.md | Backend         | Browser             | `GET /auth/linuxdo/callback` |
| User Logout                  | HTTP         | external      | New            | ./contracts/http-apis.md | Backend         | Web SPA             | `POST /api/user/logout`      |
| User Token Query             | HTTP         | external      | New            | ./contracts/http-apis.md | Backend         | Web SPA             | `GET /api/user/token`        |
| Profile optional fields      | HTTP         | external      | Modify         | ./contracts/http-apis.md | Backend         | Web SPA             | 扩展 `/api/profile`          |
| User auth persistence tables | DB           | internal      | New            | ./contracts/db.md        | Backend         | Backend             | 新增 5 张表                  |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/http-apis.md](./contracts/http-apis.md)
- [contracts/db.md](./contracts/db.md)

## 验收标准（Acceptance Criteria）

- Given 未登录访问首页
  When 页面渲染
  Then 区域①显示 Linux DO 登录按钮，区域②不自动填充。

- Given 用户通过 LinuxDo 授权成功回调
  When 返回首页
  Then 区域①隐藏，区域②自动填入 `th-...` token 并写入 hash。

- Given 用户首次登录
  When 回调完成
  Then 在 `auth_tokens` 与 `user_token_bindings` 中各新增 1 条绑定关系。

- Given 同一用户再次登录
  When 回调完成
  Then 不新增 token，仅复用原绑定。

- Given 旧前端只读取 `/api/profile` 原字段
  When 服务升级后
  Then 旧逻辑仍可正常运行，不受新增字段影响。

## 实现前置条件（Definition of Ready / Preconditions）

- LinuxDo OAuth2 端点与字段语义已确认（wiki，2025-08-17）：已满足
- 首页交互口径（①显示/隐藏与②自动填充）已确认：已满足
- token 策略（首次创建，后续不自动重建）已确认：已满足
- 回调地址多环境配置策略已确认：已满足

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: DB 表升级与 token 绑定辅助逻辑。
- Integration tests: OAuth callback、state 校验、会话读写、`/api/user/token`、`/api/profile` 扩展字段。
- Frontend checks: PublicHome 行为回归（未登录/已登录分支）。

### Quality checks

- `cargo fmt`
- `cargo clippy -- -D warnings`
- `cargo test`
- `cd web && bun run build`

## 文档更新（Docs to Update）

- `README.md`: LinuxDo 登录配置与用户 token 自动填充说明。
- `README.zh-CN.md`: 同步中文说明。
- `docs/specs/README.md`: Index 行维护。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 完成 docs/specs 迁移与 `#rg5ju` 规格冻结。
- [x] M2: 后端完成 OAuth2 + 用户会话 + 表结构 + token 绑定实现。
- [x] M3: 前端完成首页①②交互与 Linux DO 登录按钮样式。
- [x] M4: 测试与质量检查通过，文档更新完成。
- [ ] M5: 快车道交付（push + PR + checks + review-loop 收敛）。

## 方案概述（Approach, high-level）

- 采用“用户体系与管理员体系并行隔离”的方式，避免对管理员路径造成行为回归。
- 复用现有 `auth_tokens` 作为最终 token 载体，新增绑定层完成用户归属。
- OAuth2 以最小必要参数实现授权码流程，错误路径统一可诊断响应。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：LinuxDo 线上端点偶发失败会影响登录体验（需前端明确提示）。
- 风险：URL hash 携带 token 带来分享泄露风险（本次按已确认策略保留旧行为）。
- 开放问题：用户 token 失效后是否允许自助重建（当前不允许，后续再决策）。
- 假设：生产环境可正确配置 LinuxDo 应用回调地址与 client 凭据。

## 变更记录（Change log）

- 2026-02-26: 初版规格建立，冻结接口/数据模型/验收口径。
- 2026-02-26: 完成后端 OAuth2/用户会话与首页①②交互实现，补齐测试与 README 更新。

## 参考（References）

- [Linux DO Connect](https://wiki.linux.do/Community/LinuxDoConnect)
