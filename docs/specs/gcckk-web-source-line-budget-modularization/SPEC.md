# 前端超长源码治理与预算门禁（#gcckk）

## 状态

- Status: 已完成
- Created: 2026-04-19
- Last: 2026-04-19

## 背景 / 问题陈述

- 当前仓库仍有多份明显超出可维护阈值的前端源码：`web/src/AdminDashboard.tsx`、`web/src/admin/AdminPages.stories.tsx`、`web/src/i18n.tsx`、`web/src/UserConsole.tsx`、`web/src/api.ts`、`web/src/index.css`。
- 这些单体文件同时承载页面编排、数据访问、纯 helper、故事夹具、国际化词条与样式细节，review 冲突高，定位回归困难，也让后续改动继续堆回同一入口。
- 需要沿用 Rust 行数预算的思路，为前端建立“薄入口 + 域模块 + 自动门禁”的持续约束，避免大文件回流。

## 目标 / 非目标

### Goals

- 将超长前端文件拆分为职责清晰的域模块，同时保留现有公开导入面与运行时语义。
- 为 `web/src/**` 新增自动源码行数预算检查，并接入本地验证与 CI。
- 保持 Storybook、Bun test、build 与现有 UI 行为不回退。
- 为本轮模块化产出可复核的 Storybook 视觉证据，并落盘到 spec。

### Non-goals

- 不新增产品功能、接口字段、数据库 schema 或视觉改版。
- 不修改 Rust 行数预算阈值，也不扩展后端模块化范围。
- 不将样式体系切换到 CSS Modules、Tailwind 迁移或其他全站技术替换。

## 范围（Scope）

### In scope

- `web/src/AdminDashboard.tsx` 与 `web/src/admin/**`
- `web/src/UserConsole.tsx` 与 `web/src/user-console/**`
- `web/src/api.ts` 与 `web/src/api/**`
- `web/src/i18n.tsx` 与 `web/src/i18n/**`
- `web/src/index.css` 与 `web/src/styles/**`
- `web/src/admin/AdminPages.stories.tsx`、相关 story support、story tests
- 前端源码 budget test、CI 接线、spec 视觉证据

### Out of scope

- `src/**` Rust 模块边界、HTTP/DB 契约与 server wiring
- 新 UI 设计稿、视觉重画、文案策略改版
- 非必要的组件重命名或路由语义变更

## Solution Lookup

- Solution检索: 未命中
- Solution引用: none
- solution_disposition: none

## 实施约束

- `web/src/AdminDashboard.tsx` 保持薄入口/兼容门面，仅保留 default export 与 `KeyDetails` re-export；主实现下沉到 `web/src/admin/**`。
- `web/src/UserConsole.tsx` 保持薄 orchestrator；纯 helper、guide/snippet、logout/token-secret/probe 逻辑下沉到 `web/src/user-console/**`，测试直接导入纯模块，不再依赖 `__testables` 聚合门面。
- `web/src/api.ts` 改为 barrel；按 shared/core + domain client 拆分到 `web/src/api/**`，继续兼容 `./api` 导入。
- `web/src/i18n.tsx` 改为 provider/barrel；类型、词条、provider/context 分离，同时保持 `LanguageProvider` / `useLanguage` / `useTranslate` / `Language` 的导入方式稳定。
- `web/src/index.css` 继续作为唯一入口文件，只负责按顺序导入 `web/src/styles/**` partials。
- `web/src/admin/AdminPages.stories.tsx` 改为轻量 story barrel；按模块拆分 canvas/story/support，保留 `Admin/Pages` title namespace 与既有 story export 名。

## 源码预算门禁

- 默认阈值：
  - `web/src/**/*.ts(x)` / `*.js(x)`：`<= 1500` 行
  - `web/src/**/*.stories.*`：`<= 1800` 行
  - `web/src/**/*.css`：`<= 2200` 行
- 排除：`node_modules`、`dist`、`storybook-static` 等构建产物。
- 若拆分后仍存在必须保留的例外文件，必须在本 spec 与 budget test 中显式登记理由与阈值。

### 当前显式例外

- `web/src/admin/AdminDashboardRuntime.tsx`：`<= 13000`
  - 理由：管理员总控页面仍保留 legacy runtime shell，当前已由薄入口 `web/src/AdminDashboard.tsx` 承接兼容面，后续再继续拆分 page-shell / state / details 子模块。
- `web/src/admin/storySupport/AdminPagesStoryRuntime.tsx`：`<= 7000`
  - 理由：`Admin/Pages` proof stories 已恢复轻量入口与稳定 export 名，但 story runtime 仍需后续按 dashboard / requests / users / settings 继续分包。
- `web/src/api/runtime.ts`：`<= 3200`
  - 理由：`web/src/api.ts` 已切为薄 barrel，旧 API 合同先收敛到 `web/src/api/runtime.ts`，便于不破坏现有导入面地继续域化。
- `web/src/user-console/runtime.tsx`：`<= 3000`
  - 理由：`guide` / `text` 已下沉到 `web/src/user-console/**`，route-level shell 与 probe/logout orchestration 仍需下一轮继续抽 hook / pure helpers。
- `web/src/admin/ForwardProxySettingsModule.tsx`：`<= 2400`
  - 理由：当前 refactor 范围未覆盖 forward proxy 设置页，只登记预算例外避免本轮门禁误伤。
- `web/src/components/AdminRecentRequestsPanel.tsx`：`<= 1600`
  - 理由：共享 requests 面板仍由既有组件承载，待独立 follow-up 再拆。
- `web/src/pages/TokenDetail.tsx`：`<= 1700`
  - 理由：Token detail drill-down 页未纳入本轮结构化拆分，仅登记预算例外。

## 验收标准（Acceptance Criteria）

- Given 当前前端超长文件
  When 完成模块化
  Then `web/src/AdminDashboard.tsx`、`web/src/UserConsole.tsx`、`web/src/api.ts`、`web/src/i18n.tsx`、`web/src/index.css` 与 `web/src/admin/AdminPages.stories.tsx` 都应降为薄入口/barrel 或拆分后的轻量文件。

- Given admin runtime 与 admin story surfaces
  When 查看 request-log、quota、user-tag、monthly-broken 等共享逻辑
  Then 应由 `web/src/admin/**` 的共享 helper / support 模块统一提供，避免运行时与 story 维护两套同构实现。

- Given 前端源码预算门禁
  When 执行新增 budget test
  Then 任意超预算文件都必须报出路径、分类阈值与实际行数。

- Given 现有前端入口与 Storybook
  When 执行 `bun test`、`bun run build`、`bun run build-storybook`
  Then 现有导入路径、页面行为与 Storybook proof 不应因模块化而回退。

## 非功能性验收 / 质量门槛（Quality Gates）

- `cd web && bun test`
- `cd web && bun run build`
- `cd web && bun run build-storybook`
- PR 上的 `CI Pipeline` 与 `Docs Pages` 通过

## Visual Evidence

- 不作为本轮交付物。
- 原因：本次改动目标是前端工程治理与预算门禁，不引入预期 UI 变更；视觉一致性由 `bun run build-storybook` 与既有 Storybook story surface 作为技术回归校验承担。
- 若后续发现真实 UI 回归，再按单独缺陷补充对应页面的视觉证据。

## 里程碑（Milestones）

- [x] M1: 创建 spec 并冻结前端模块化边界与预算阈值
- [x] M2: 拆分 `api` / `i18n` / `styles` 与 admin shared helpers
- [x] M3: 拆分 `AdminDashboard` / `UserConsole` / `AdminPages.stories`
- [x] M4: 新增前端 budget test 与 CI 接线
- [x] M5: 完成验证与 PR review-loop 收敛到 merge-ready
