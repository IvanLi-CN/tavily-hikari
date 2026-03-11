# 管理员控制台返回用户控制台入口（#mx657）

## 状态

- Status: 已完成
- Created: 2026-03-09
- Last: 2026-03-09

## 背景 / 问题陈述

- 管理员已经可以从用户控制台进入系统管理控制台，但 `/admin` 侧当前缺少对等的返回入口。
- 管理员进入 `/admin` 后若想回到 `/console`，只能手工修改地址，破坏双控制台往返体验。
- 现有 admin 页头与 detail/leaderboard 页头样式分散，若各自临时补按钮，容易造成文案、图标与移动端布局漂移。

## 目标 / 非目标

### Goals

- 在 admin 所有顶层页头变体中提供统一的“返回用户控制台” CTA。
- CTA 始终显示，固定跳转 `/console`，不记忆先前 hash 子路由。
- 复用共享前端 primitive，确保 dashboard/list/detail/leaderboard 的入口文案、图标、ARIA 与 href 一致。
- 补齐桌面与移动端布局适配，避免新入口挤掉现有 theme/language/refresh/back 等控件。
- 补齐组件样例与自动化断言，锁定按钮文本与跳转目标。

### Non-goals

- 不新增侧栏入口、面包屑入口或额外的会话切换逻辑。
- 不修改 `/console`、`/admin`、`/login`、`/` 的既有鉴权与重定向策略。
- 不新增后端 API、数据库字段或 Rust handler。
- 不实现“返回到用户控制台最近访问的 token/detail hash”。

## 范围（Scope）

### In scope

- `web/src/components/AdminPanelHeader.tsx`
- `web/src/components/TokenUsageHeader.tsx`
- `web/src/pages/TokenDetail.tsx`
- `web/src/AdminDashboard.tsx`
- `web/src/i18n.tsx`
- `web/src/index.css`
- 新增 admin 返回用户控制台共享组件/常量与对应测试、stories
- `docs/specs/README.md`

### Out of scope

- Rust 后端、HTTP 契约与 `/api/profile` 数据结构。
- 用户控制台 `/console` 页头与 PublicHome 首页入口行为。

## 接口契约（Interfaces & Contracts）

- 新增前端内部常量：admin 返回用户控制台目标固定为 `/console`。
- 共享 admin header 组件需支持注入或直接渲染该 CTA，保证所有 admin 顶层页头语义一致。
- 不新增或修改任何 public HTTP 接口。

## 验收标准（Acceptance Criteria）

- Given 管理员访问 `/admin`、`/admin/tokens`、`/admin/keys`、`/admin/requests`、`/admin/jobs`
  When 页面完成渲染
  Then 共享页头显示“返回用户控制台”入口，点击进入 `/console`。

- Given 管理员访问 `/admin/tokens/leaderboard`、`/admin/tokens/:id`、`/admin/keys/:id`、`/admin/users/:id`
  When 页面完成渲染
  Then 顶层页头或详情头区域同样显示统一 CTA，图标、文案、href 与 dashboard 页头一致。

- Given 页面处于 stacked mobile breakpoint
  When 新 CTA 出现
  Then theme、language、refresh、back、regenerate secret 等既有控件仍可见且不重叠。

- Given 用户点击该 CTA
  When 浏览器跳转到 `/console`
  Then 沿用现有用户控制台默认行为，不新增 hash、query 或额外中间跳转。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- `cd web && bun test`
- `cd web && bun run build`

### UI / Browser

- Storybook 或组件样例可直接预览 dashboard-style header 与 detail-style header 的返回入口。
- 真实浏览器验证 `/admin`、`/admin/tokens/leaderboard`、`/admin/tokens/:id`、`/admin/keys/:id`、`/admin/users/:id` 均存在该 CTA。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 创建 admin 返回用户控制台共享 primitive 与固定 href 常量
- [x] M2: dashboard/list header 与 leaderboard header 接入统一 CTA
- [x] M3: token detail / key detail / user detail 顶部区域接入统一 CTA
- [x] M4: i18n、responsive CSS、stories 与自动化断言补齐
- [x] M5: build、浏览器验收、快车道 PR 收敛完成

## 风险 / 开放问题 / 假设

- 风险：admin 移动端页头高度较紧，新 CTA 若处理不当会挤压 refresh/back 控件。
- 开放问题：GitHub 未自动为截图 docs 提交触发新的 pull_request workflow；已补跑 `CI Pipeline` workflow_dispatch（run `#372`）并确认通过，当前无剩余代码阻塞。
- 假设：管理员在 `/admin` 下始终允许直接访问 `/console`，无需再判断是否存在有效用户 session。
- 假设：固定返回 `/console` 即可满足当前产品诉求，本轮不追踪最近一次用户控制台子路由。

## Visual Evidence (PR)

![管理控制台页头展示返回用户控制台入口](./assets/admin-dashboard-return-to-console.png)

![用户控制台落地页保持既有 fallback](./assets/user-console-landing-fallback.png)

## 变更记录（Change log）

- 2026-03-09: 创建 spec，冻结“页头全局、始终显示、固定跳转 `/console`、不改后端接口”的实现边界。
- 2026-03-09: 已完成共享 return CTA、admin header/detail 接入、i18n 与 responsive 调整；通过 `cd web && bun test`、`cd web && bun run build`，并在本地浏览器验证 `/admin`、`/admin/tokens/leaderboard`、`/admin/tokens/demo-token`、`/admin/keys/demo-key`、`/admin/users/demo-user` 的入口与 `/console` 跳转。
- 2026-03-09: PR `#108` 已创建；`codex review --base origin/main` 已完成且无高优先级阻塞项，本地 `bun test` / `bun run build` 与浏览器验收通过。
- 2026-03-09: 补充管理界面与用户控制台落地页截图到 spec 资产，作为 PR 视觉验收证据；GitHub 自动 pull_request checks 未为该 docs 提交生成新 run，已补跑 `CI Pipeline` workflow_dispatch（run `#372`）并通过。
