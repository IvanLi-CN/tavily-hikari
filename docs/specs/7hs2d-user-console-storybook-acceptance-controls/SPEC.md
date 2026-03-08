# UserConsole Storybook 验收参数重构（#7hs2d）

## 状态

- Status: 已完成（快车道）
- Created: 2026-03-07
- Last: 2026-03-08

## 背景 / 问题陈述

- `web/src/UserConsole.stories.tsx` 当前把 `scenario` 作为公开 Storybook 参数暴露给验收者。
- `scenario` 是内部 mock 聚合开关，不是验收者应理解的业务语义，导致 Controls 与 preset stories 需要依赖内部场景名。
- 现有 Storybook 虽已覆盖 dashboard、tokens、token detail 与 probe 特殊态，但切换路径偏向开发实现便利，而不是验收直观性。

## 目标 / 非目标

### Goals

- 移除 `scenario` 这类面向实现的内部聚合参数在 UserConsole Storybook 验收界面的暴露。
- 以验收者可直接理解的业务状态提供切换能力，覆盖 dashboard、tokens、token detail 与需保留的特殊展示态。
- 保留故事内部 mock、hash 路由与自动 probe 驱动能力，但将其收敛为实现细节，不再要求验收者理解。

### Non-goals

- 不修改 `web/src/UserConsole.tsx` 的运行时产品功能与真实 UI 逻辑。
- 不修改后端接口、鉴权逻辑、`/console` 或 `/admin` 线上路由行为。
- 不在 Storybook 中补造管理员/非管理员差异；当前 UserConsole 实页无对应可见差异，本轮明确排除。

## 范围（Scope）

### In scope

- `web/src/UserConsole.stories.tsx`
  - 将公开 args 改为验收语义：`consoleView`、`tokenListState`、`tokenDetailPreview`
  - 用 `argTypes` 条件控制隐藏无关参数
  - 将 preset stories 改名为业务可理解的验收入口
- `docs/specs/7hs2d-user-console-storybook-acceptance-controls/SPEC.md`
- `docs/specs/README.md`

### Out of scope

- `web/src/UserConsole.tsx`、`src/**`、运行时接口契约与真实页面行为。
- UserConsole 中管理员入口显示/隐藏的新增设计与实现。

## 接口契约（Interfaces & Contracts）

### Public / acceptance-facing interfaces

- Storybook UserConsole stories 公开 controls 仅保留：
  - `consoleView`: `Dashboard | Tokens | Token Detail`
  - `tokenListState`: `Default List | Empty`
  - `tokenDetailPreview`: `Overview | API Check Running | All Checks Pass | Partial Availability | Authentication Failed | Quota Blocked`

### Internal interfaces

- 允许 Storybook 内部继续使用 hash 路由、fetch mock、auto-probe 与 probe mode resolver。
- 内部 resolver 不得再通过公开 `scenario` 或同类聚合参数暴露给验收者。

## 验收标准（Acceptance Criteria）

- Given 验收者打开 UserConsole 的 Storybook
  When 查看 Controls 或 preset stories
  Then 不再出现 `scenario` 这类内部参数名。

- Given 验收者希望查看不同控制台页面
  When 切换 `consoleView`
  Then 可直接查看 `Dashboard`、`Tokens`、`Token Detail`，无需输入内部场景码或手改 hash。

- Given 验收者希望查看 token 列表空态
  When `consoleView=Tokens` 且 `tokenListState=Empty`
  Then 可直接看到空列表展示。

- Given 验收者希望查看 token detail 特殊展示态
  When 切换 `tokenDetailPreview`
  Then 可直接预览 `Overview`、`API Check Running`、`All Checks Pass`、`Partial Availability`、`Authentication Failed`、`Quota Blocked`。

- Given 验收者通过侧边栏选择 preset stories
  When 打开目标 story
  Then story 名称与显示效果一一对应，不需要记忆内部 `scenario` 命名。

- Given 本轮实现完成
  When 运行前端质量门槛
  Then `cd web && bun run build` 与 `cd web && bun run build-storybook` 均通过。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- `cd web && bun test src/UserConsole.stories.test.ts src/lib/mcpProbe.test.js`
- `cd web && bun run build`
- `cd web && bun run build-storybook`

### UI / Storybook

- Controls 仅展示与当前 `consoleView` 相关的业务参数。
- Docs/Controls/URL args 中不存在 `scenario`。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新建独立 spec 并冻结验收范围
- [x] M2: UserConsole stories 公开 args 重构为业务语义
- [x] M3: 条件 controls 与 preset stories 验收命名收敛
- [x] M4: build + Storybook build 验证通过
- [x] M5: fast-track 交付与 review-loop 收敛

## 风险 / 开放问题 / 假设

- 风险：Storybook 条件 controls 若配置不当，可能仍在 Docs/Controls 中显示与当前页面无关的参数。
- 假设：当前 token detail probe 的 5 类特殊态仍是本轮需要保留的全部验收特殊态。
- 假设：管理员/非管理员差异本轮不纳入 UserConsole Storybook 验收。

## 变更记录（Change log）

- 2026-03-07: 创建 spec，冻结 Storybook 验收参数重构范围；明确移除 `scenario` 公开暴露，并排除管理员差异项。

- 2026-03-07: 将 UserConsole Storybook 公开 args 收敛为 `consoleView`、`tokenListState`、`tokenDetailPreview`，并把 preset stories 改为业务语义命名。
- 2026-03-07: 已完成 `cd web && bun run build` 与 `cd web && bun run build-storybook`；静态产物确认 UserConsole stories 不再暴露 `scenario`。
- 2026-03-07: 新增 `web/src/UserConsole.stories.test.ts`，回归锁定 acceptance-facing args、条件 controls 与旧导出名移除。
- 2026-03-08: 完成 PR #102 与最新 `main` 的冲突收敛，补跑 Label Gate / CI Pipeline 全部成功，PR 恢复为 `mergeable_state=clean`。
