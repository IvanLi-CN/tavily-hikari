# UserConsole Storybook 验收参数重构（#7hs2d）

## 状态

- Status: 已完成（快车道）
- Created: 2026-03-07
- Last: 2026-03-18

## 背景 / 问题陈述

- `web/src/UserConsole.stories.tsx` 当前把 `scenario` 作为公开 Storybook 参数暴露给验收者。
- `scenario` 是内部 mock 聚合开关，不是验收者应理解的业务语义，导致 Controls 与 preset stories 需要依赖内部场景名。
- 现有 Storybook 虽已覆盖 landing、token detail 与 probe 特殊态，但 probe 特殊态继续挂在整页 UserConsole stories 下，会让验收入口噪音回流。

## 目标 / 非目标

### Goals

- 移除 `scenario` 这类面向实现的内部聚合参数在 UserConsole Storybook 验收界面的暴露。
- 以验收者可直接理解的业务状态提供切换能力，覆盖 landing、token detail 与需保留的特殊展示态。
- 让整页 UserConsole stories 只保留页面级别的状态切换，把 probe 多状态收敛到独立 fragment gallery。
- 保留故事内部 mock 与 hash 路由能力，但将其收敛为实现细节，不再要求验收者理解。

### Non-goals

- 不修改 `web/src/UserConsole.tsx` 的运行时产品功能与真实 UI 逻辑。
- 不修改后端接口、鉴权逻辑、`/console` 或 `/admin` 线上路由行为。
- 不在 Storybook 中补造管理员/非管理员差异；当前 UserConsole 实页无对应可见差异，本轮明确排除。

## 范围（Scope）

### In scope

- `web/src/UserConsole.stories.tsx`
  - 将公开 args 改为验收语义：`consoleView`、`landingFocus`、`tokenListState`、`tokenDetailPreview`
  - 用 `argTypes` 条件控制隐藏无关参数
  - 将 preset stories 改名为业务可理解的验收入口，并删除 probe-only 的全页 story
- `web/src/components/ConnectivityChecksPanel.tsx`
- `web/src/components/ConnectivityChecksPanel.stories.tsx`
- `web/src/components/ConnectivityChecksPanel.stories.test.ts`
- `docs/specs/7hs2d-user-console-storybook-acceptance-controls/SPEC.md`
- `docs/specs/README.md`

### Out of scope

- `src/**`、运行时接口契约与真实页面行为。
- UserConsole 中管理员入口显示/隐藏的新增设计与实现。

## 接口契约（Interfaces & Contracts）

### Public / acceptance-facing interfaces

- Storybook UserConsole stories 公开 controls 仅保留：
  - `consoleView`: `Console Home | Token Detail`
  - `landingFocus`: `Overview Focus | Token Focus`
  - `tokenListState`: `Single Token | Multiple Tokens | Empty`
  - `tokenDetailPreview`: `Overview | Token Revealed`
- Storybook probe 特殊态通过独立 `User Console/Fragments/Connectivity Checks/State Gallery` 聚合展示。

### Internal interfaces

- 允许 Storybook 内部继续使用 hash 路由与 fetch mock。
- probe 状态不得再通过公开 `tokenDetailPreview` 或同类整页聚合参数暴露给验收者。

## 验收标准（Acceptance Criteria）

- Given 验收者打开 UserConsole 的 Storybook
  When 查看 Controls 或 preset stories
  Then 不再出现 `scenario` 这类内部参数名。

- Given 验收者希望查看不同控制台页面
  When 切换 `consoleView`
  Then 可直接查看 `Console Home` 与 `Token Detail`，无需输入内部场景码或手改 hash。

- Given 验收者希望查看 token 列表空态或多 token 列表
  When `consoleView=Console Home` 且切换 `tokenListState`
  Then 可直接看到空列表展示。

- Given 验收者希望查看 token detail 页面级展示态
  When 切换 `tokenDetailPreview`
  Then 可直接预览 `Overview` 与 `Token Revealed`。

- Given 验收者希望集中查看 probe 多状态
  When 打开 `User Console/Fragments/Connectivity Checks/State Gallery`
  Then 可在一个独立界面中同时查看 idle、running、success、partial、authentication failed 与 quota blocked，并看到 MCP `tools/list` 后对全部广告工具逐个 `tools/call` 的展示效果。

- Given 验收者通过侧边栏选择 preset stories
  When 打开目标 story
  Then story 名称与显示效果一一对应，不需要记忆内部 `scenario` 命名。

- Given 本轮实现完成
  When 运行前端质量门槛
  Then `cd web && bun run build` 与 `cd web && bun run build-storybook` 均通过。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- `cd web && bun test src/UserConsole.stories.test.ts src/components/ConnectivityChecksPanel.stories.test.ts src/lib/mcpProbe.test.js`
- `cd web && bun run build`
- `cd web && bun run build-storybook`

### UI / Storybook

- Controls 仅展示与当前 `consoleView` 相关的业务参数。
- Docs/Controls/URL args 中不存在 `scenario`。
- probe 状态矩阵通过独立 fragment gallery 展示，不再新增 probe-only 的整页 UserConsole story。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新建独立 spec 并冻结验收范围
- [x] M2: UserConsole stories 公开 args 重构为业务语义
- [x] M3: 条件 controls 与 preset stories 验收命名收敛
- [x] M4: build + Storybook build 验证通过
- [x] M5: fast-track 交付与 review-loop 收敛

## 风险 / 开放问题 / 假设

- 风险：Storybook 条件 controls 若配置不当，可能仍在 Docs/Controls 中显示与当前页面无关的参数。
- 风险：若 `ConnectivityChecksPanel` 与 `UserConsole` 后续分叉，fragment gallery 可能出现“看起来对、页面里不对”的漂移。
- 假设：probe 的多状态验收更适合 fragment gallery，而不是整页页面 Story。
- 假设：管理员/非管理员差异本轮不纳入 UserConsole Storybook 验收。

## Visual Evidence (PR)

Connectivity Checks gallery Canvas，聚合展示 idle、running、success、partial、authentication failed、quota blocked，并明确呈现 MCP `tools/list` 后对全部广告工具逐个 `tools/call` 的展示效果。

![Connectivity Checks gallery canvas](./assets/connectivity-checks-gallery-canvas.png)

## 变更记录（Change log）

- 2026-03-07: 创建 spec，冻结 Storybook 验收参数重构范围；明确移除 `scenario` 公开暴露，并排除管理员差异项。

- 2026-03-07: 将 UserConsole Storybook 公开 args 收敛为 `consoleView`、`tokenListState`、`tokenDetailPreview`，并把 preset stories 改为业务语义命名。
- 2026-03-07: 已完成 `cd web && bun run build` 与 `cd web && bun run build-storybook`；静态产物确认 UserConsole stories 不再暴露 `scenario`。
- 2026-03-07: 新增 `web/src/UserConsole.stories.test.ts`，回归锁定 acceptance-facing args、条件 controls 与旧导出名移除。
- 2026-03-08: 完成 PR #102 与最新 `main` 的冲突收敛，补跑 Label Gate / CI Pipeline 全部成功，PR 恢复为 `mergeable_state=clean`。
- 2026-03-18: 跟进 45squ probe 展示收口；整页 `UserConsole` stories 仅保留页面级态，新增独立 `Connectivity Checks` fragment gallery 聚合 probe 多状态，并把 MCP `tools/list` 后的全部工具调用展示纳入同一验收面板，同时同步更新测试与文档口径。
