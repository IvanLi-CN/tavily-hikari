# 用户控制台 Path 路由迁移（#8yzmy）

## 状态

- Status: 已完成（快车道）
- Created: 2026-04-12
- Last: 2026-04-13

## 背景 / 问题陈述

- 当前 `/console` 仍依赖 `#/dashboard`、`#/tokens` 与 `#/tokens/:id` 承载用户控制台路由，不符合前台 path 路由要求。
- 开发态与生产态目前只保证 `/console` / `/console/` 入口，直接打开 `/console/*` 深链会落回 404 或错误入口。
- 首页 `/` 已存在用 `#token` / `#token-id` 承载 access token 恢复的历史行为；这部分若被一并迁到 path/query，会放大日志与分享泄露风险。

## 目标 / 非目标

### Goals

- 将用户控制台路由合同升级为 path：`/console`、`/console/dashboard`、`/console/tokens`、`/console/tokens/:id`。
- 用手写 history 同步替换现有 hash 同步：路由解析、前进/后退、section 定位、detail 返回都只认 path。
- 开发态 Vite 与 Rust 静态托管都支持任意 `/console/*` 深链直开。
- 保留首页 `/` 的 `#token` / `#token-id` 兼容行为，不迁移到 path/query。
- 明确旧 `/console#/...` 深链不再兼容，运行时不做自动迁移或双栈支持。

### Non-goals

- 不改 `/api/*`、鉴权、用户配额与 Token 数据契约。
- 不把首页 token 恢复机制迁移到 path/query，也不把完整 token 放进 URL path/query。
- 不引入新的前端路由框架。
- 不兼容旧 `/console#/...` 深链。

## 范围（Scope）

### In scope

- `web/src/UserConsole.tsx`
- `web/src/lib/userConsoleRoutes.ts`
- `web/src/lib/userConsoleRoutes.test.ts`
- `web/src/UserConsole.test.ts`
- `web/src/UserConsole.stories.tsx`
- `web/src/UserConsole.stories.test.ts`
- `web/vite.config.ts`
- `src/server/serve.rs`
- `README.md`
- `docs/specs/README.md`

### Out of scope

- `web/src/PublicHome.tsx` 的交互重设计（只允许补兼容回归测试/说明）。
- `/admin` 路由、管理端 Storybook、后端业务 handler。
- PublicHome token 存储模型改造。

## 需求（Requirements）

### MUST

- 用户控制台只按 path 解析路由；hash 不再驱动 `/console` 视图切换。
- `/console` 默认进入 merged landing；`/console/dashboard` 与 `/console/tokens` 分别定位到对应 landing section。
- `/console/tokens/:id` 保持 token detail 页；返回 Token 列表后进入 `/console/tokens`。
- 直接访问任意 `/console/*` 深链时，dev/build 模式都能命中 `console.html`。
- 首页 `/` 必须继续兼容旧 `#token` / `#token-id` 恢复链路。
- Storybook 与自动化断言需覆盖 `/console`、`/console/dashboard`、`/console/tokens`、`/console/tokens/:id` 四类状态。

### SHOULD

- 路径归一化后尽量收敛到无尾斜杠 canonical path。
- logout fallback 与 console-only deployment fallback 应接受嵌套 `/console/*` 路径。

### COULD

- 对无效 `/console/*` path 做前端 canonical replace，统一回到 `/console` 或 `/console/tokens`。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 用户访问 `/console`
  - 进入 merged landing 默认视图。
  - 页面 route state 为 `{ name: 'landing', section: null }`。
- 用户访问 `/console/dashboard`
  - 进入 merged landing。
  - 页面自动聚焦到账户概览区块。
- 用户访问 `/console/tokens`
  - 进入 merged landing。
  - 页面自动聚焦到 Token 列表区块。
- 用户访问 `/console/tokens/:id`
  - 进入 token detail 页。
  - 浏览器 Back/Forward 保持 path 历史往返。
- 用户访问 `/console#/dashboard`、`/console#/tokens` 或 `/console#/tokens/:id`
  - 页面只按 `/console` 根路径渲染默认 landing。
  - hash 不参与 route 解析，也不触发自动迁移。
- 用户访问首页 `/`
  - 若 URL hash 为完整 token 或 token id，仍沿用现有恢复逻辑。
  - token 仅保留在 hash / localStorage，不进入 path/query。

### Edge cases / errors

- 若 `/console/*` path 无法解析为已知视图，前端回退到 `/console` 默认 landing，并在同一文档会话里 canonical replace 到合法 path。
- 若 token detail path 中的 token id 解码失败，则回退到 `/console/tokens`。
- 若首页 hash 不可解析为完整 token 或 token id，仍沿用当前无 token 回退语义。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                   | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers）                    | 备注（Notes）                 |
| ------------------------------ | ------------ | ------------- | -------------- | ------------------------ | --------------- | -------------------------------------- | ----------------------------- |
| `/console` 路由合同            | route-state  | internal      | Modify         | None                     | web             | UserConsole, Storybook, static hosting | 从 hash 改为 path             |
| `/console/*` 静态托管 fallback | http-route   | internal      | Modify         | None                     | web+server      | 浏览器、Vite、Axum static serving      | 任意深链都返回 `console.html` |
| 首页 `#token` / `#token-id`    | route-state  | internal      | None           | None                     | web             | PublicHome                             | 显式保留的安全兼容例外        |

### 契约文档（按 Kind 拆分）

None

## 验收标准（Acceptance Criteria）

- Given 用户直接访问 `/console`
  When 页面渲染完成
  Then merged landing 正常显示，且地址不需要 hash。

- Given 用户直接访问 `/console/dashboard`
  When 页面渲染完成
  Then merged landing 自动定位到账户概览区块。

- Given 用户直接访问 `/console/tokens`
  When 页面渲染完成
  Then merged landing 自动定位到 Token 列表区块。

- Given 用户直接访问 `/console/tokens/a1b2`
  When 页面渲染完成
  Then token detail 正常显示，且返回/前进后退维持 path 历史。

- Given 用户访问 `/console#/tokens/a1b2`
  When 页面渲染完成
  Then 控制台只按 `/console` 根路径渲染默认 landing，不再进入旧 hash detail 语义。

- Given 用户访问首页 `/#th-a1b2-1234567890abcdef` 或 `/#a1b2`
  When PublicHome 初始化完成
  Then 仍按旧逻辑恢复 token 体验，且完整 token 不进入 path/query。

- Given 实现完成
  When 运行前端验证
  Then `cd web && bun test`、`cd web && bun run build`、`cd web && bun run build-storybook` 通过。

## 实现前置条件（Definition of Ready / Preconditions）

- `/console` 新 path 契约与首页 hash 兼容边界已冻结。
- legacy `/console#/...` 不兼容为显式决策。
- 本轮沿用手写 history sync，不引入新路由框架。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: `cd web && bun test`
- Build: `cd web && bun run build`
- Storybook build: `cd web && bun run build-storybook`

### UI / Storybook (if applicable)

- Stories to add/update: `web/src/UserConsole.stories.tsx`
- Docs pages / state galleries to add/update: none（沿用现有 story gallery 结构）
- `play` / interaction coverage to add/update: none（当前页面 stories 以 mock state gallery + story test 为主）

### Quality checks

- `cargo test`

## 文档更新（Docs to Update）

- `README.md`: 同步当前 `/console` path 路由合同，并注明首页 `#token` 兼容保留。
- `docs/specs/README.md`: 索引新增并回写状态。

## 计划资产（Plan assets）

- Directory: `docs/specs/8yzmy-user-console-path-routing/assets/`
- In-plan references: `![...](./assets/<file>.png)`
- Visual evidence source: maintain `## Visual Evidence` in this spec when owner-facing or PR-facing screenshots are needed.

## Visual Evidence

- 本次任务不要求提交截图资产；路径迁移以 `cd web && bun test`、`cd web && bun run build`、`cd web && bun run build-storybook`、`cargo test` 与本地 Storybook 路由复核为准。

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新建 follow-up spec 并冻结 `/console` path 路由与首页 hash 兼容边界
- [x] M2: `UserConsole` 改为 path-only 路由解析与 history 同步
- [x] M3: Vite / Rust 静态托管支持 `/console/*` 深链
- [x] M4: Storybook、单测与首页 hash 回归覆盖更新
- [ ] M5: 视觉证据、验证、PR 与 spec-sync 收口到 PR-ready

## 方案概述（Approach, high-level）

- 用轻量 path helper 取代现有 hash helper，统一运行时与 Storybook 的 route fixture。
- 控制台内部继续保留 merged landing + token detail 两种 view，只把 URL contract 从 hash 切到 path。
- 首页 hash 作为显式安全例外保留，不与 `/console` 的 path 改造绑定迁移。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：Storybook 预览 iframe 使用 path fixture 时，若 history 操作不收敛，可能影响 story 切换稳定性。
- 风险：`/console/*` fallback 若 dev/build 任一侧漏掉，会出现“本地可用、部署后 404”的不一致。
- 假设：当前 merged landing 的 section 语义保持不变，仅改 URL 载体。

## 变更记录（Change log）

- 2026-04-12: 创建 follow-up spec，冻结 `/console` path 路由合同、首页 `#token` 保留兼容、以及 legacy `/console#/...` 明确不兼容的边界。
- 2026-04-12: 完成 `UserConsole` path route helper、`popstate` history sync、Vite/Rust `/console/*` fallback，以及首页 `#token` / `#token-id` 兼容回归测试。
- 2026-04-12: 完成 `cd web && bun test`、`cd web && bun run build`、`cd web && bun run build-storybook`、`cargo test` 本地验证；按当前任务口径不提交截图资产，PR 以路由契约、测试与构建验证为主。
- 2026-04-13: 补齐 `type:patch` + `channel:stable` release labels，并将 spec / 索引同步到可合并完成态，准备进入 merge + cleanup。

## 参考（References）

- `docs/specs/2nx74-user-console-single-page-landing/SPEC.md`
- `docs/specs/m4n7x-admin-path-routing-modular-dashboard/SPEC.md`
- `docs/specs/rg5ju-linuxdo-login-token-autofill/SPEC.md`
