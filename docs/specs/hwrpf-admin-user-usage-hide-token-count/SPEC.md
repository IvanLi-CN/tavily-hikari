# Admin 用户用量页隐藏令牌数列（#hwrpf）

## 状态

- Status: 已完成
- Created: 2026-03-22
- Last: 2026-03-22

## 背景 / 问题陈述

- `/admin/users/usage` 当前同时展示“状态”和“令牌数”，但主人明确要求这个页面不再显示令牌数。
- 该列只存在于展示层：桌面表格、移动端卡片，以及 Storybook 中复刻该页面的画布实现。
- 如果仅删除 JSX 而不同步修正空态列数和固定列宽，页面会出现表头/内容宽度错位。

## 目标 / 非目标

### Goals

- 从管理端用户用量页桌面表格中移除“令牌数”表头与对应单元格。
- 从同页移动端卡片中移除“令牌数”字段。
- 同步修正用户用量表的空态 `colSpan` 与 9 列固定宽度映射，并让 Storybook 画布与生产页保持一致。

### Non-goals

- 不改 `/api/users` 返回的 `tokenCount` 字段，不改 Rust DTO、SQL 聚合或接口测试。
- 不删除用户详情页中的 `tokenCount` 展示。
- 不新增新的排序字段、测试框架或页面交互。

## 范围（Scope）

### In scope

- `web/src/AdminDashboard.tsx`
  - 删除用户用量页桌面/移动端中的令牌数展示。
  - 将空态 `colSpan` 从 `10` 调整为 `9`。
- `web/src/index.css`
  - 将 `admin-users-usage-table` 的固定列宽规则从 10 列重排为 9 列。
- `web/src/admin/AdminPages.stories.tsx`
  - 让 Users Usage Storybook 画布与生产页列结构一致。

### Out of scope

- `src/**` 后端逻辑、`web/src/api.ts` 类型定义与 `tokenCount` 数据契约。
- 用户详情页、普通用户列表页、token 列表页或其他任何复用 `tokenCount` 的视图。

## 需求（Requirements）

### MUST

- 用户用量页不再出现 `令牌数 / Tokens` 文案与数值。
- 桌面表格与移动端卡片必须同步去掉该字段，避免响应式口径不一致。
- 表格删列后仍需保持固定布局可读，不得出现空列、错位或异常压缩。

### SHOULD

- Storybook 画布继续作为该页面的人工验收来源，并与生产实现保持一一对应。

### COULD

- None.

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 管理员访问 `/admin/users/usage` 时，桌面表格直接从“状态”列跳到“1h（任意）”列，中间不再出现令牌数列。
- 管理员在移动端查看同页时，卡片字段列表不再显示“令牌数”，其余配额与成功率字段顺序保持不变。
- 当列表为空时，表格空态使用新的 9 列宽度占满整行，不出现边框断裂或占位偏移。

### Edge cases / errors

- 即使 `/api/users` 仍返回 `tokenCount`，前端也只是不渲染该值，不影响现有数据请求与排序参数。
- Storybook mock 仍可保留 `tokenCount` 数据，但该数据不得在 Users Usage 画布中继续显示。

## 接口契约（Interfaces & Contracts）

- None.

## 验收标准（Acceptance Criteria）

- Given 管理员进入 `/admin/users/usage`
  When 页面渲染桌面表格
  Then 表头与各行都不再显示“令牌数 / Tokens”列。

- Given 管理员在移动端查看 `/admin/users/usage`
  When 用户用量卡片渲染
  Then 卡片内不再显示“令牌数 / Tokens”字段。

- Given 用户用量页返回空列表
  When 空态渲染
  Then 表格占位与列数一致，不出现错位。

- Given Storybook 打开 Users Usage 画布
  When 画布渲染
  Then 列结构与生产页一致，且不再显示令牌数列。

## 实现前置条件（Definition of Ready / Preconditions）

- 主人已确认本次只收缩前端展示范围。
- `tokenCount` 后端字段保留，不纳入本轮契约变更。
- 需要联动的文件范围已定位：`AdminDashboard.tsx`、`index.css`、`AdminPages.stories.tsx`。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Frontend build: `cd web && bun run build`

### UI / Storybook (if applicable)

- Stories to add/update: `web/src/admin/AdminPages.stories.tsx` 中的 Users Usage 画布。

### Quality checks

- 保持 TypeScript 构建通过，不引入新的 lint/type 错误。

## 文档更新（Docs to Update）

- `docs/specs/README.md`: 新增该 spec 索引，并在实现完成后同步状态与备注。

## 计划资产（Plan assets）

- Directory: `docs/specs/hwrpf-admin-user-usage-hide-token-count/assets/`
- In-plan references: none
- PR visual evidence source: 如 PR 需要截图，再补充到本目录。

## Visual Evidence (PR)

## 资产晋升（Asset promotion）

- None.

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 冻结“仅移除用户用量页令牌数展示”的规格边界
- [x] M2: 删除生产页与 Storybook 中的令牌数列/字段，并同步修正空态与列宽
- [x] M3: 完成构建验证、review 收敛与 PR 交付准备

## 方案概述（Approach, high-level）

- 保持接口层不动，只在用户用量页渲染链路中移除该字段。
- 以 CSS 固定列宽重排兜底，避免删列后表格宽度沿用旧的 `nth-child` 映射。
- Storybook 直接复刻生产页结构，作为回归与人工验收的最小镜像。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：若只删 JSX 不改 `nth-child` 宽度规则，会造成列宽错位。
- 需要决策的问题：None.
- 假设（需主人确认）：主人所说“去掉这列”仅针对 `/admin/users/usage`，不影响用户详情页。

## 变更记录（Change log）

- 2026-03-22: 创建 spec，冻结“仅移除管理端用户用量页令牌数展示”的范围与验收口径。
- 2026-03-22: 生产页与 Users Usage Storybook 画布已移除令牌数列；`bun run build` 与 `bun test src/admin/routes.test.ts src/api.test.ts` 通过。
- 2026-03-22: PR #175 已创建，release labels 已补齐，`codex review --base origin/main` 未发现需修复回归。

## 参考（References）

- `web/src/AdminDashboard.tsx`
- `web/src/admin/AdminPages.stories.tsx`
- `web/src/index.css`
